/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use core::borrow;
use std::{
    ops::Index,
    sync::{Arc, Mutex, Weak},
};

use crate::{
    api::{
        Internal, LzmaIndexIter, LzmaIndexIterMode, LzmaRet, LzmaStreamFlags, LzmaVli,
        LZMA_BACKWARD_SIZE_MAX, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_MAX, LZMA_VLI_UNKNOWN,
    },
    common::{
        IndexGroup, IndexNode, IndexRecord, IndexStream, IndexTree, IndexTreeNode, LzmaIndex,
        NodeType,
    },
};

use super::lzma_vli_size;

/// 最小未填充大小
pub const UNPADDED_SIZE_MIN: LzmaVli = 5;

/// 最大未填充大小
pub const UNPADDED_SIZE_MAX: LzmaVli = LZMA_VLI_MAX & !3;

/// 根据 xz 规范的索引指示符
pub const INDEX_INDICATOR: u8 = 0;

/// 将可变长度整数向上舍入到四的倍数
pub fn vli_ceil4(vli: LzmaVli) -> LzmaVli {
    assert!(vli <= LZMA_VLI_MAX);
    (vli + 3) & !3
}

/// 计算索引字段的大小（不包括索引填充）
pub fn index_size_unpadded(count: LzmaVli, index_list_size: LzmaVli) -> LzmaVli {
    // 索引指示符 + 记录数量 + 记录列表 + CRC32
    1 + lzma_vli_size(count) as LzmaVli + index_list_size + 4
}

/// 计算索引字段的大小（包括索引填充）
pub fn index_size(count: LzmaVli, index_list_size: LzmaVli) -> LzmaVli {
    vli_ceil4(index_size_unpadded(count, index_list_size))
}

/// 计算流的总大小
pub fn index_stream_size(
    blocks_size: LzmaVli,
    count: LzmaVli,
    index_list_size: LzmaVli,
) -> LzmaVli {
    LZMA_STREAM_HEADER_SIZE as u64
        + blocks_size
        + index_size(count, index_list_size)
        + LZMA_STREAM_HEADER_SIZE as u64
}

/// 但又不能过大，以避免浪费过多的内存。
pub const INDEX_GROUP_SIZE: usize = 512;

/// 允许分配的最大记录数量
pub const PREALLOC_MAX: usize =
    (usize::MAX - std::mem::size_of::<IndexGroup>()) / std::mem::size_of::<IndexRecord>();

/// 初始化索引树
fn index_tree_init(tree: &mut IndexTree) {
    tree.root = None;
    tree.leftmost = None;
    tree.rightmost = None;
    tree.count = 0;
}

/// 索引树节点结束的辅助函数
fn index_tree_node_end(node: &mut IndexTreeNode, free_func: Option<fn(&mut IndexTreeNode)>) {
    // 如果节点有左子树，则递归处理
    if let Some(mut left) = node.get_left() {
        let mut left_refs = left.lock().unwrap();
        index_tree_node_end(left_refs.get_tree_node_mut(), free_func);
    }

    // 如果节点有右子树，则递归处理
    if let Some(mut right) = node.get_right() {
        let mut right_refs = right.lock().unwrap();

        index_tree_node_end(right_refs.get_tree_node_mut(), free_func);
    }

    match free_func {
        Some(f) => f(node),
        None => {}
    }
}

/// 释放为树分配的内存。每个节点都使用给定的 free_func 进行释放，
/// free_func 可以是 lzma_free 或 index_stream_end。
/// 后者用于在释放 index_stream 本身之前释放每个 index_stream 中的 Record 组。
fn index_tree_end(tree: &mut IndexTree, free_func: Option<fn(&mut IndexTreeNode)>) {
    if let Some(ref root) = tree.root {
        let mut root_refs = root.lock().unwrap();
        index_tree_node_end(root_refs.get_tree_node_mut(), free_func);
    }
}

/// 向索引树添加一个新节点（顺序插入，带 AVL 平衡）
fn index_tree_append(tree: &mut IndexTree, index_node: Arc<Mutex<IndexNode>>) {
    // 设置父节点、左右子节点
    {
        let mut index_node_refs = index_node.lock().unwrap();
        let mut node = index_node_refs.get_tree_node_mut();

        node.parent = tree.rightmost.as_ref().map(Arc::downgrade);
        node.left = None;
        node.right = None;

        tree.count += 1;

        // 处理添加第一个节点的特殊情况
        if tree.root.is_none() {
            tree.set_root(Some(index_node.clone()));
            tree.set_leftmost(Some(index_node.clone()));
            tree.set_rightmost(Some(index_node.clone()));

            return;
        }
        {
            let mut tree_rightmost = tree.rightmost.as_ref().unwrap();
            let mut tree_rightmost_refs = tree_rightmost.lock().unwrap();

            assert!(tree_rightmost_refs.uncompressed_base() <= node.uncompressed_base);
            assert!(tree_rightmost_refs.compressed_base() < node.compressed_base);

            let mut rightmost_node = tree_rightmost_refs.get_tree_node_mut();

            rightmost_node.set_right(Some(index_node.clone()));
        }

        tree.rightmost = Some(index_node.clone());
    }

    // AVL 平衡（顺序插入时的特殊处理）
    let up = tree.count ^ (1 << (tree.count as u32).leading_zeros());
    if up != 0 {
        let mut up = tree.count.trailing_zeros() + 2;
        let mut cur = index_node;

        while up > 1 {
            let parent = {
                let cur_borrow = cur.lock().unwrap();
                cur_borrow.get_tree_node().get_parent()
            };
            cur = parent.unwrap();
            up -= 1;
        }

        // `cur` is now the rotation root.
        let pivot = {
            let mut cur_borrow = cur.lock().unwrap();
            cur_borrow.get_tree_node_mut().get_right()
        };
        let parent = {
            let cur_borrow = cur.lock().unwrap();
            cur_borrow.get_tree_node().get_parent()
        };

        if let Some(pivot_arc) = pivot {
            // Step 1: Link parent to pivot
            match parent.as_ref() {
                Some(parent_arc) => {
                    let mut parent_borrow = parent_arc.lock().unwrap();
                    parent_borrow
                        .get_tree_node_mut()
                        .set_right(Some(pivot_arc.clone()));
                }
                None => {
                    tree.set_root(Some(pivot_arc.clone()));
                }
            }

            let pivot_left_child;

            // Steps 2, 6: Link pivot to parent, set pivot's left child
            {
                let mut pivot_borrow = pivot_arc.lock().unwrap();
                let pivot_node = pivot_borrow.get_tree_node_mut();

                pivot_node.set_parent(parent);
                pivot_left_child = pivot_node.get_left();
                pivot_node.set_left(Some(cur.clone()));
            }

            // Steps 4, 5, 7: Update cur and pivot's original left child
            {
                if let Some(plc_arc) = pivot_left_child.as_ref() {
                    let mut plc_borrow = plc_arc.lock().unwrap();
                    plc_borrow.get_tree_node_mut().set_parent(Some(cur.clone()));
                }

                let mut cur_borrow = cur.lock().unwrap();
                let cur_node = cur_borrow.get_tree_node_mut();

                cur_node.set_right(pivot_left_child);
                cur_node.set_parent(Some(pivot_arc.clone()));
            }
        }
    }
}

/// 获取树中的下一个节点（中序遍历后继）。
pub fn index_tree_next(node: &Arc<Mutex<IndexNode>>) -> Option<Arc<Mutex<IndexNode>>> {
    // 1. 如果有右子树，后继是右子树的最左节点
    if let Some(mut next) = node.lock().unwrap().get_tree_node().get_right() {
        loop {
            let left = next.lock().unwrap().get_tree_node().get_left();
            match left {
                Some(left_node) => next = left_node,
                None => break,
            }
        }
        return Some(next);
    }
    // 2. 否则，向上找第一个不是父节点右孩子的祖先

    let mut cur = node.clone();

    loop {
        let parent_arc = match cur.lock().unwrap().get_tree_node().get_parent() {
            Some(p) => p,
            None => break,
        };

        let is_right_child = {
            let parent_borrow = parent_arc.lock().unwrap();
            if let Some(right) = parent_borrow.get_tree_node().get_right() {
                Arc::ptr_eq(&right, &cur)
            } else {
                false
            }
        };
        if is_right_child {
            cur = parent_arc;
            continue;
        } else {
            return Some(parent_arc);
        }
    }

    None
}

/// 查找包含给定解压缩偏移量的节点。
/// 调用者需要确保 `target` 不超过树的解压缩总大小（在这种情况下将返回最后一个节点）。
fn index_tree_locate(tree: &IndexTree, target: LzmaVli) -> Option<Arc<Mutex<IndexNode>>> {
    let mut result = None;
    let mut node = tree.root.clone();

    // 确保 leftmost 节点的解压缩基址为 0（如同 C 中的 assert）
    assert!(
        tree.leftmost.is_none()
            || tree
                .leftmost
                .as_ref()
                .map_or(false, |n| n.lock().unwrap().uncompressed_base() == 0)
    );

    while let Some(ref cur) = node {
        let cur_base = {
            let cur_borrow = cur.lock().unwrap();
            cur_borrow.uncompressed_base()
        };
        if cur_base > target {
            node = {
                let cur_borrow = cur.lock().unwrap();
                cur_borrow.get_tree_node().get_left()
            };
        } else {
            result = Some(cur.clone());
            node = {
                let cur_borrow = cur.lock().unwrap();
                cur_borrow.get_tree_node().get_right()
            };
        }
    }
    result
}

/// 使用给定的基准偏移量分配并初始化一个新的 Stream。
fn index_stream_init(
    compressed_base: LzmaVli,
    uncompressed_base: LzmaVli,
    stream_number: u32,
    block_number_base: LzmaVli,
) -> Option<Arc<Mutex<IndexNode>>> {
    let stream = IndexStream {
        node: IndexTreeNode::new(uncompressed_base, compressed_base),
        number: stream_number,
        block_number_base,
        groups: IndexTree::default(),
        record_count: 0,
        index_list_size: 0,
        stream_flags: LzmaStreamFlags {
            version: u32::MAX,
            ..Default::default()
        },
        stream_padding: 0,
    };
    Some(Arc::new(Mutex::new(IndexNode::Stream(stream))))
}

/// 释放分配给 Stream 及其记录组的内存。
fn index_stream_end(node: Arc<Mutex<IndexStream>>) {
    // 结束并释放 groups 相关资源
    let mut s = node.lock().unwrap();
    index_tree_end(&mut s.groups, None);
}

// 初始化一个空的 lzma_index。
fn index_init_plain() -> Arc<Mutex<LzmaIndex>> {
    Arc::new(Mutex::new(LzmaIndex::new()))
}

/// 初始化一个 lzma_index，分配并初始化相关资源。
pub fn lzma_index_init() -> Option<Arc<Mutex<LzmaIndex>>> {
    let mut i = index_init_plain();
    // 初始化一个 stream
    let s = index_stream_init(0, 0, 1, 0);

    match s {
        Some(stream_node) => {
            let mut lzma_index = i.lock().unwrap();
            index_tree_append(&mut lzma_index.streams, stream_node);
        }
        None => {}
    }

    Some(i)
}

/// 适配器：将 &mut IndexTreeNode 转为 IndexStream 并调用 index_stream_end
fn index_stream_end_adapter(node: &mut IndexTreeNode) {
    // 安全前提：node 必须实际属于 IndexStream
    use std::mem;
    let stream_ptr = node as *mut IndexTreeNode as *mut IndexStream;
    unsafe {
        // 直接 clone 并包裹 Arc/Mutex 调用 index_stream_end
        index_stream_end(Arc::new(Mutex::new((*stream_ptr).clone())));
    }
}

/// 释放 lzma_index 相关的资源。
pub fn lzma_index_end(i: &mut LzmaIndex) {
    index_tree_end(&mut i.streams, Some(index_stream_end_adapter));
}

/// 设置 lzma_index 的预分配大小。
pub fn lzma_index_prealloc(i: Arc<Mutex<LzmaIndex>>, records: LzmaVli) {
    // const PREALLOC_MAX: u64 = 1024;  // 假设的常量，根据实际情况调整

    let records: LzmaVli = if records > PREALLOC_MAX as u64 {
        PREALLOC_MAX as LzmaVli
    } else {
        records
    };

    let mut i_refs = i.lock().unwrap();

    i_refs.prealloc = records as usize;
}

/// 计算 lzma_index 的内存使用量。
pub fn lzma_index_memusage(streams: LzmaVli, blocks: LzmaVli) -> LzmaVli {
    // 计算 malloc() 的额外开销
    let alloc_overhead = 4 * std::mem::size_of::<*const u8>();

    // 每个 Stream 基本结构体所需的内存
    let stream_base =
        std::mem::size_of::<IndexStream>() + std::mem::size_of::<IndexGroup>() + 2 * alloc_overhead;

    // 每个 Group 所需的内存
    let group_base = std::mem::size_of::<IndexGroup>()
        + INDEX_GROUP_SIZE * std::mem::size_of::<IndexRecord>()
        + alloc_overhead;

    // 计算需要的 Group 数量
    let groups = (blocks + INDEX_GROUP_SIZE as u64 - 1) / INDEX_GROUP_SIZE as u64;

    // 计算各个结构的内存占用
    let streams_mem = streams * stream_base as u64;
    let groups_mem = groups * group_base as u64;

    // 基本结构体所需的内存
    let index_base = std::mem::size_of::<LzmaIndex>() + alloc_overhead;

    // 验证参数有效性并处理溢出情况
    let limit = u64::MAX - index_base as u64;
    if streams == 0
        || streams > u32::MAX as u64
        || blocks > LZMA_VLI_MAX
        || streams > limit / stream_base as u64
        || groups > limit / group_base as u64
        || limit - streams_mem < groups_mem
    {
        return u64::MAX; // 内存溢出，返回最大值
    }

    // 返回内存使用量的总和
    index_base as u64 + streams_mem + groups_mem
}

/// 计算并返回 lzma_index 使用的内存量。
pub fn lzma_index_memused(i: Arc<Mutex<LzmaIndex>>) -> LzmaVli {
    let i_refs = i.lock().unwrap();

    let stream_ref = &i_refs.streams;
    lzma_index_memusage(stream_ref.count as u64, i_refs.record_count)
}

/// 获取 lzma_index 的块计数。
pub fn lzma_index_block_count(i: Arc<Mutex<LzmaIndex>>) -> LzmaVli {
    let i_refs = i.lock().unwrap();
    i_refs.record_count
}

/// 获取 lzma_index 的流计数。
pub fn lzma_index_stream_count(i: Arc<Mutex<LzmaIndex>>) -> LzmaVli {
    let i_refs = i.lock().unwrap();

    let stream_ref = &i_refs.streams;

    stream_ref.count as LzmaVli
}

/// 获取 lzma_index 的大小。
pub fn lzma_index_size(i: &LzmaIndex) -> LzmaVli {
    index_size(i.record_count, i.index_list_size)
}

/// 获取 lzma_index 的总大小。
pub fn lzma_index_total_size(i: &LzmaIndex) -> LzmaVli {
    i.total_size
}

/// 获取 lzma_index 流的大小。
pub fn lzma_index_stream_size(i: &LzmaIndex) -> LzmaVli {
    LZMA_STREAM_HEADER_SIZE as LzmaVli
        + i.total_size
        + index_size(i.record_count, i.index_list_size)
        + LZMA_STREAM_HEADER_SIZE as LzmaVli
}

/// 计算索引文件的大小。
fn index_file_size(
    compressed_base: LzmaVli,
    unpadded_sum: LzmaVli,
    record_count: LzmaVli,
    index_list_size: LzmaVli,
    stream_padding: LzmaVli,
) -> LzmaVli {
    // const LZMA_VLI_MAX: u64 = u64::MAX;
    // const LZMA_STREAM_HEADER_SIZE: u64 = 12;  // 假设常量，根据实际情况调整

    let mut file_size = compressed_base
        + 2 * LZMA_STREAM_HEADER_SIZE as LzmaVli
        + stream_padding
        + vli_ceil4(unpadded_sum);
    if file_size > LZMA_VLI_MAX {
        return u64::MAX; // 表示文件大小溢出
    }

    file_size += index_size(record_count, index_list_size);
    if file_size > LZMA_VLI_MAX {
        return u64::MAX; // 表示文件大小溢出
    }

    file_size
}

/// 获取 lzma_index 的文件大小。
pub fn lzma_index_file_size(i: Arc<Mutex<LzmaIndex>>) -> LzmaVli {
    let i_ref = i.lock().unwrap();
    let streams_tree = &i_ref.streams;

    // Get the rightmost stream node
    if let Some(s_node_arc) = streams_tree.rightmost.as_ref() {
        let s_node_borrow = s_node_arc.lock().unwrap();

        // Ensure it's a stream and get the stream data
        if let Some(stream) = s_node_borrow.as_stream() {
            let groups_tree = &stream.groups;

            let unpadded_sum = if let Some(g_node_arc) = groups_tree.rightmost.as_ref() {
                let g_node_borrow = g_node_arc.lock().unwrap();
                // Ensure it's a group and get the group data
                if let Some(group) = g_node_borrow.as_group() {
                    // Check if there are records and `last` is a valid index
                    if !group.records.is_empty() && group.last < group.records.len() {
                        group.records[group.last].unpadded_sum
                    } else {
                        0 // No records or invalid last index
                    }
                } else {
                    0 // Rightmost node of groups tree is not a group, should not happen
                }
            } else {
                0 // No groups in the stream
            };

            return index_file_size(
                stream.node.compressed_base,
                unpadded_sum,
                stream.record_count,
                stream.index_list_size,
                stream.stream_padding,
            );
        }
    }

    0
}

/// 获取 lzma_index 的未压缩大小。
pub fn lzma_index_uncompressed_size(i: &LzmaIndex) -> LzmaVli {
    i.uncompressed_size
}

/// 获取 lzma_index 的校验类型。
pub fn lzma_index_checks(i: &LzmaIndex) -> u32 {
    let mut checks = i.checks;
    if let Some(s_node_arc) = i.streams.rightmost.as_ref() {
        let s_node_borrow = s_node_arc.lock().unwrap();
        if let Some(stream) = s_node_borrow.as_stream() {
            if stream.stream_flags.version != u32::MAX {
                checks |= 1 << (stream.stream_flags.check as u32);
            }
        }
    }
    checks
}

/// 获取 lzma_index 的填充大小。
pub fn lzma_index_padding_size(i: &LzmaIndex) -> u32 {
    let unpadded_size = index_size_unpadded(i.record_count, i.index_list_size);
    let padding_size = (4 - (unpadded_size % 4)) % 4;

    padding_size as u32
}

/// 设置 lzma_index 的流标志。
pub fn lzma_index_stream_flags(i: &mut LzmaIndex, stream_flags: &LzmaStreamFlags) -> LzmaRet {
    if stream_flags.version == u32::MAX {
        return LzmaRet::ProgError;
    }
    if let Some(s_node_arc) = i.streams.rightmost.as_ref() {
        let mut s_node_borrow = s_node_arc.lock().unwrap();
        if let Some(stream) = s_node_borrow.as_stream_mut() {
            stream.stream_flags = stream_flags.clone();
            return LzmaRet::Ok;
        }
    }
    LzmaRet::ProgError
}

/// 设置 lzma_index 的流填充。
pub fn lzma_index_stream_padding(i: &Arc<Mutex<LzmaIndex>>, stream_padding: LzmaVli) -> LzmaRet {
    if stream_padding > LZMA_VLI_MAX || (stream_padding & 3) != 0 {
        return LzmaRet::ProgError;
    }
    let s_node_arc = {
        let i_borrow = i.lock().unwrap();
        i_borrow.streams.rightmost.clone()
    };
    if let Some(s_node_arc) = s_node_arc {
        let old_stream_padding;
        {
            let mut s_node_borrow = s_node_arc.lock().unwrap();
            if let Some(stream) = s_node_borrow.as_stream_mut() {
                old_stream_padding = stream.stream_padding;
                stream.stream_padding = 0;
            } else {
                return LzmaRet::ProgError;
            }
        }
        if lzma_index_file_size(i.clone()) + stream_padding > LZMA_VLI_MAX {
            let mut s_node_borrow = s_node_arc.lock().unwrap();
            if let Some(stream) = s_node_borrow.as_stream_mut() {
                stream.stream_padding = old_stream_padding;
            }
            return LzmaRet::DataError;
        }
        {
            let mut s_node_borrow = s_node_arc.lock().unwrap();
            if let Some(stream) = s_node_borrow.as_stream_mut() {
                stream.stream_padding = stream_padding;
            }
        }
        return LzmaRet::Ok;
    }
    LzmaRet::ProgError
}

/// 向 lzma_index 添加一个新的记录。
pub fn lzma_index_append(i: &mut LzmaIndex, unpadded_size: u64, uncompressed_size: u64) -> LzmaRet {
    // Validate
    if unpadded_size < UNPADDED_SIZE_MIN
        || unpadded_size > UNPADDED_SIZE_MAX
        || uncompressed_size > LZMA_VLI_MAX
    {
        return LzmaRet::ProgError;
    }
    // 获取当前流
    let s_node_arc = match i.streams.rightmost.as_ref() {
        Some(arc) => arc.clone(),
        None => return LzmaRet::ProgError,
    };
    let mut s_node = s_node_arc.lock().unwrap();
    let stream = match s_node.as_stream_mut() {
        Some(s) => s,
        None => return LzmaRet::ProgError,
    };
    // 获取当前组
    let mut g_node_arc_opt = stream.groups.rightmost.as_ref().cloned();
    let (compressed_base, uncompressed_base) = if let Some(ref g_node_arc) = g_node_arc_opt {
        let g_node = g_node_arc.lock().unwrap();
        if let Some(group) = g_node.as_group() {
            if !group.records.is_empty() && group.last < group.records.len() {
                (
                    vli_ceil4(group.records[group.last].unpadded_sum),
                    group.records[group.last].uncompressed_sum,
                )
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        }
    } else {
        (0, 0)
    };
    let index_list_size_add = lzma_vli_size(unpadded_size) + lzma_vli_size(uncompressed_size);
    // 检查未压缩大小是否会溢出
    if uncompressed_base + uncompressed_size > LZMA_VLI_MAX {
        return LzmaRet::DataError;
    }
    // 检查新的未填充和文件大小是否会溢出
    if compressed_base + unpadded_size > UNPADDED_SIZE_MAX {
        return LzmaRet::DataError;
    }
    if index_file_size(
        stream.node.compressed_base,
        compressed_base + unpadded_size,
        stream.record_count + 1,
        stream.index_list_size + index_list_size_add as u64,
        stream.stream_padding,
    ) == LZMA_VLI_UNKNOWN
    {
        return LzmaRet::DataError;
    }
    if index_size(
        i.record_count + 1,
        i.index_list_size + index_list_size_add as u64,
    ) > LZMA_BACKWARD_SIZE_MAX
    {
        return LzmaRet::DataError;
    }
    // 检查是否有空间添加新记录
    let mut group_arc;
    let mut group_last;
    if let Some(ref g_node_arc) = g_node_arc_opt {
        let mut g_node = g_node_arc.lock().unwrap();
        let group = g_node.as_group_mut().unwrap();
        if group.last + 1 < group.allocated {
            group.last += 1;
            group_arc = g_node_arc.clone();
            group_last = group.last;
        } else {
            // 新建组
            let mut new_group = IndexGroup::new(i.prealloc.max(1));
            new_group.node.uncompressed_base = uncompressed_base;
            new_group.node.compressed_base = compressed_base;
            new_group.number_base = stream.record_count + 1;
            new_group.allocated = i.prealloc.max(1);
            new_group.last = 0;
            new_group.records.push(IndexRecord {
                uncompressed_sum: 0,
                unpadded_sum: 0,
            });
            let new_group_arc = Arc::new(Mutex::new(IndexNode::Group(new_group)));
            index_tree_append(&mut stream.groups, new_group_arc.clone());
            i.prealloc = INDEX_GROUP_SIZE;
            group_arc = new_group_arc;
            group_last = 0;
        }
    } else {
        // 新建第一个组
        let mut group = IndexGroup::new(i.prealloc.max(1));
        group.node.uncompressed_base = 0;
        group.node.compressed_base = 0;
        group.number_base = 1;
        group.allocated = i.prealloc.max(1);
        group.last = 0;
        group.records.push(IndexRecord {
            uncompressed_sum: 0,
            unpadded_sum: 0,
        });
        let new_group_arc = Arc::new(Mutex::new(IndexNode::Group(group)));
        index_tree_append(&mut stream.groups, new_group_arc.clone());
        i.prealloc = INDEX_GROUP_SIZE;
        group_arc = new_group_arc;
        group_last = 0;
    }
    // 添加新记录
    {
        let mut g_node = group_arc.lock().unwrap();
        let group = g_node.as_group_mut().unwrap();
        if group.records.len() <= group_last {
            group.records.push(IndexRecord {
                uncompressed_sum: 0,
                unpadded_sum: 0,
            });
        }
        group.records[group_last].uncompressed_sum = uncompressed_base + uncompressed_size;
        group.records[group_last].unpadded_sum = compressed_base + unpadded_size;
    }
    // 更新计数
    stream.record_count += 1;
    stream.index_list_size += index_list_size_add as u64;
    i.total_size += vli_ceil4(unpadded_size);
    i.uncompressed_size += uncompressed_size;
    i.record_count += 1;
    i.index_list_size += index_list_size_add as u64;
    LzmaRet::Ok
}

/// 用于传递信息给 index_cat_helper() 的结构体
pub struct IndexCatInfo {
    /// 目标的未压缩大小
    pub uncompressed_size: LzmaVli,

    /// 目标的压缩文件大小
    pub file_size: LzmaVli,

    /// 对应块编号的大小
    pub block_number_add: LzmaVli,

    /// 在开始从源索引追加新的流之前，目标索引中的流数
    /// 用于修正流编号
    pub stream_number_add: u32,

    /// 目标索引的流树
    pub streams: Option<Arc<Mutex<IndexTree>>>,
}

/// 通过递归将源索引中的流节点添加到目标索引
/// 最简单的源树迭代遍历无法工作
/// 因为在将节点移动到目标树时我们需要更新节点中的指针
fn index_cat_helper(info: &IndexCatInfo, stream_arc: &Arc<Mutex<IndexNode>>) {
    let mut stream_borrow = stream_arc.lock().unwrap();
    let stream = match stream_borrow.as_stream_mut() {
        Some(s) => s,
        None => return,
    };
    // 递归处理左子树
    if let Some(left_arc) = stream.node.get_left() {
        index_cat_helper(info, &left_arc);
    }
    // 更新当前节点信息
    stream.node.uncompressed_base += info.uncompressed_size;
    stream.node.compressed_base += info.file_size;
    stream.number += info.stream_number_add;
    stream.block_number_base += info.block_number_add;
    // 添加到目标树
    if let Some(ref streams_arc) = info.streams {
        index_tree_append(&mut streams_arc.lock().unwrap(), stream_arc.clone());
    }
    // 递归处理右子树
    if let Some(right_arc) = stream.node.get_right() {
        index_cat_helper(info, &right_arc);
    }
}

/// 将源索引中的所有流添加到目标索引中。
pub fn lzma_index_cat(dest: &mut LzmaIndex, src: &mut LzmaIndex) -> LzmaRet {
    let dest_file_size = lzma_index_file_size(Arc::new(Mutex::new(dest.clone())));
    if dest_file_size + lzma_index_file_size(Arc::new(Mutex::new(src.clone()))) > LZMA_VLI_MAX
        || dest.uncompressed_size + src.uncompressed_size > LZMA_VLI_MAX
    {
        return LzmaRet::DataError;
    }
    let dest_size = index_size_unpadded(dest.record_count, dest.index_list_size);
    let src_size = index_size_unpadded(src.record_count, src.index_list_size);
    if vli_ceil4(dest_size + src_size) > LZMA_BACKWARD_SIZE_MAX {
        return LzmaRet::DataError;
    }

    if let Some(s_node_arc) = dest.streams.rightmost.as_ref().cloned() {
        let mut s_node = s_node_arc.lock().unwrap();
        if let Some(stream) = s_node.as_stream_mut() {
            if let Some(g_node_arc) = stream.groups.rightmost.as_ref().cloned() {
                let mut g_node = g_node_arc.lock().unwrap();
                if let Some(group) = g_node.as_group_mut() {
                    if group.last + 1 < group.allocated {
                        // 新建更小的 group
                        let mut new_group = group.clone();
                        new_group.allocated = group.last + 1;
                        new_group.records.truncate(new_group.allocated);
                        // 保持 node/parent/left/right/number_base
                        let new_group_arc = Arc::new(Mutex::new(IndexNode::Group(new_group)));
                        // 替换 parent 的 right 指针
                        if let Some(parent_weak) = &group.node.parent {
                            if let Some(parent_arc) = parent_weak.upgrade() {
                                let mut parent = parent_arc.lock().unwrap();
                                parent
                                    .get_tree_node_mut()
                                    .set_right(Some(new_group_arc.clone()));
                            }
                        }
                        // 替换 leftmost/root/rightmost
                        if let Some(leftmost_arc) = &stream.groups.leftmost {
                            if Arc::ptr_eq(leftmost_arc, &g_node_arc) {
                                stream.groups.leftmost = Some(new_group_arc.clone());
                            }
                        }
                        if let Some(root_arc) = &stream.groups.root {
                            if Arc::ptr_eq(root_arc, &g_node_arc) {
                                stream.groups.root = Some(new_group_arc.clone());
                            }
                        }
                        stream.groups.rightmost = Some(new_group_arc);
                        // 原 group Arc 会自动 drop
                    }
                }
            }
        }
    }

    dest.checks = lzma_index_checks(dest);
    // 遍历 src.streams，递归插入到 dest.streams
    let info = IndexCatInfo {
        uncompressed_size: dest.uncompressed_size,
        file_size: dest_file_size,
        stream_number_add: dest.streams.count,
        block_number_add: dest.record_count,
        streams: Some(Arc::new(Mutex::new(dest.streams.clone()))),
    };
    if let Some(root_arc) = src.streams.root.as_ref() {
        index_cat_helper(&info, root_arc);
    }
    dest.uncompressed_size += src.uncompressed_size;
    dest.total_size += src.total_size;
    dest.record_count += src.record_count;
    dest.index_list_size += src.index_list_size;
    dest.checks |= src.checks;

    // 优化最后一个 group 以最小化内存使用

    LzmaRet::Ok
}

fn index_dup_stream(src_arc: &Arc<Mutex<IndexNode>>) -> Option<Arc<Mutex<IndexNode>>> {
    let src_borrow = src_arc.lock().unwrap();
    let src_stream = src_borrow.as_stream()?;
    if src_stream.record_count > PREALLOC_MAX as u64 {
        return None;
    }
    // 新建 stream
    let mut new_stream = src_stream.clone();
    new_stream.groups = IndexTree::default();
    // 如果没有 group，直接返回
    if src_stream.groups.leftmost.is_none() {
        return Some(Arc::new(Mutex::new(IndexNode::Stream(new_stream))));
    }
    // 合并所有 record 到一个新 group
    let mut all_records = Vec::with_capacity(src_stream.record_count as usize);
    let mut srcg_opt = src_stream.groups.leftmost.clone();
    while let Some(srcg_arc) = srcg_opt {
        let srcg_borrow = srcg_arc.lock().unwrap();
        // 只要是 group 节点就合并
        match &*srcg_borrow {
            IndexNode::Group(srcg) => {
                if srcg.last < srcg.records.len() {
                    all_records.extend_from_slice(&srcg.records[..=srcg.last]);
                }
            }
            _ => {}
        }
        srcg_opt = srcg_borrow.get_tree_node().get_right();
    }
    assert_eq!(all_records.len(), src_stream.record_count as usize);
    // 新 group
    let mut destg = IndexGroup::new(all_records.len());
    destg.node.uncompressed_base = 0;
    destg.node.compressed_base = 0;
    destg.number_base = 1;
    destg.allocated = all_records.len();
    destg.last = all_records.len() - 1;
    destg.records = all_records;
    let destg_arc = Arc::new(Mutex::new(IndexNode::Group(destg)));
    index_tree_append(&mut new_stream.groups, destg_arc);
    Some(Arc::new(Mutex::new(IndexNode::Stream(new_stream))))
}

pub fn lzma_index_dup(src: &LzmaIndex) -> Option<LzmaIndex> {
    let mut dest = LzmaIndex::new();
    dest.uncompressed_size = src.uncompressed_size;
    dest.total_size = src.total_size;
    dest.record_count = src.record_count;
    dest.index_list_size = src.index_list_size;
    dest.prealloc = src.prealloc;
    dest.checks = src.checks;
    // 复制所有 stream
    let mut stream_arc_opt = src.streams.leftmost.clone();
    while let Some(stream_arc) = stream_arc_opt {
        let new_stream_arc = index_dup_stream(&stream_arc)?;
        index_tree_append(&mut dest.streams, new_stream_arc);
        stream_arc_opt = stream_arc.lock().unwrap().get_tree_node().get_right();
    }
    Some(dest)
}

const ITER_INDEX: usize = 0;
const ITER_STREAM: usize = 1;
const ITER_GROUP: usize = 2;
const ITER_RECORD: usize = 3;
const ITER_METHOD: usize = 4;
const ITER_METHOD_NORMAL: usize = 0;
const ITER_METHOD_NEXT: usize = 1;
const ITER_METHOD_LEFTMOST: usize = 2;
pub fn iter_set_info(iter: &mut LzmaIndexIter) {
    use crate::api::Internal;
    use crate::common::{IndexGroup, IndexStream};
    use std::rc::Rc;

    // 1. 先 clone 出所有需要的值，避免后续 set 时冲突
    let i = match iter.internal.get(ITER_INDEX).unwrap().clone() {
        Internal::Index(i) => i,
        _ => panic!("iter_set_info: not an index"),
    };
    let stream = match iter.internal.get(ITER_STREAM).unwrap().clone() {
        Internal::Stream(s) => s,
        _ => panic!("iter_set_info: not a stream"),
    };
    let group = match iter.internal.get(ITER_GROUP).unwrap().clone() {
        Internal::Group(g) => Some(g),
        Internal::None => None,
        _ => panic!("iter_set_info: not a group"),
    };
    let record = match iter.internal.get(ITER_RECORD).unwrap().clone() {
        Internal::Size(s) => s,
        _ => panic!("iter_set_info: not a record"),
    };
    let stream_ref = stream.as_ref();
    let group_ref = group.as_ref().map(|g| g.as_ref());

    // 2. 判？断 group 情况，设置 ITER_METHOD 和 ITER_GROUP
    if group_ref.is_none() {
        // 没有组
        assert!(stream_ref.groups.root.is_none());
        iter.internal
            .set(ITER_METHOD, Internal::Size(ITER_METHOD_LEFTMOST))
            .unwrap();
    } else {
        let group = group_ref.unwrap();
        // 判断是否为最后一个 group
        let is_last_group = i.streams.rightmost.as_ref().map_or(false, |arc| {
            if let Some(s) = arc.lock().unwrap().as_stream() {
                if let Some(s_arc) = stream_ref.groups.rightmost.as_ref() {
                    Arc::ptr_eq(arc, s_arc)
                } else {
                    false
                }
            } else {
                false
            }
        }) && stream_ref.groups.rightmost.as_ref().map_or(false, |arc| {
            if let Some(g) = arc.lock().unwrap().as_group() {
                if let Some(g_arc) = stream_ref.groups.rightmost.as_ref() {
                    Arc::ptr_eq(arc, g_arc)
                } else {
                    false
                }
            } else {
                false
            }
        });
        if is_last_group {
            iter.internal
                .set(ITER_METHOD, Internal::Size(ITER_METHOD_NORMAL))
                .unwrap();
        } else {
            // 判断是否为唯一 group
            let is_only_group = stream_ref.groups.leftmost.as_ref().map_or(false, |arc| {
                if let Some(g) = arc.lock().unwrap().as_group() {
                    if let Some(g_arc) = stream_ref.groups.leftmost.as_ref() {
                        Arc::ptr_eq(arc, g_arc)
                    } else {
                        false
                    }
                } else {
                    false
                }
            });
            if !is_only_group {
                assert!(stream_ref.groups.root.as_ref().map_or(true, |arc| {
                    if let Some(g) = arc.lock().unwrap().as_group() {
                        if let Some(g_arc) = stream_ref.groups.root.as_ref() {
                            !Arc::ptr_eq(arc, g_arc)
                        } else {
                            true
                        }
                    } else {
                        true
                    }
                }));
                let parent_arc = group
                    .node
                    .parent
                    .as_ref()
                    .and_then(|w| w.upgrade())
                    .expect("group must have parent");
                let parent_borrow = parent_arc.lock().unwrap();
                let right_is_group =
                    parent_borrow
                        .get_tree_node()
                        .get_right()
                        .map_or(false, |arc| {
                            if let Some(g) = arc.lock().unwrap().as_group() {
                                if let Some(g_arc) = stream_ref.groups.rightmost.as_ref() {
                                    Arc::ptr_eq(&arc, &g_arc)
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        });
                assert!(right_is_group);
                iter.internal
                    .set(ITER_METHOD, Internal::Size(ITER_METHOD_NEXT))
                    .unwrap();
                if let Some(parent_group) = parent_borrow.as_group() {
                    iter.internal
                        .set(ITER_GROUP, Internal::Group(Box::new(parent_group.clone())))
                        .unwrap();
                } else {
                    iter.internal.set(ITER_GROUP, Internal::None).unwrap();
                }
            } else {
                assert!(stream_ref.groups.root.as_ref().map_or(false, |arc| {
                    if let Some(g) = arc.lock().unwrap().as_group() {
                        if let Some(g_arc) = stream_ref.groups.root.as_ref() {
                            Arc::ptr_eq(arc, g_arc)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }));
                assert!(group.node.parent.is_none());
                iter.internal
                    .set(ITER_METHOD, Internal::Size(ITER_METHOD_LEFTMOST))
                    .unwrap();
                iter.internal.set(ITER_GROUP, Internal::None).unwrap();
            }
        }
    }

    // 3. 设置 stream 信息
    iter.stream.number = stream_ref.number as u64;
    iter.stream.block_count = stream_ref.record_count;
    iter.stream.compressed_offset = stream_ref.node.compressed_base;
    iter.stream.uncompressed_offset = stream_ref.node.uncompressed_base;
    iter.stream.flags = if stream_ref.stream_flags.version == u32::MAX {
        None
    } else {
        Some(stream_ref.stream_flags.clone())
    };
    iter.stream.padding = stream_ref.stream_padding;

    // 4. 设置 stream 的 compressed/uncompressed_size
    if stream_ref.groups.rightmost.is_none() {
        iter.stream.compressed_size = index_size(0, 0) + 2 * LZMA_STREAM_HEADER_SIZE as u64;
        iter.stream.uncompressed_size = 0;
    } else {
        let g_arc = stream_ref.groups.rightmost.as_ref().unwrap();
        let g_borrow = g_arc.lock().unwrap();
        let g = g_borrow.as_group().unwrap();
        iter.stream.compressed_size = 2 * LZMA_STREAM_HEADER_SIZE as u64
            + index_size(stream_ref.record_count, stream_ref.index_list_size)
            + vli_ceil4(g.records[g.last].unpadded_sum);
        iter.stream.uncompressed_size = g.records[g.last].uncompressed_sum;
    }

    // 5. 如果 group 存在，设置 block 信息
    if let Some(group) = group_ref {
        iter.block.number_in_stream = group.number_base + record as u64;
        iter.block.number_in_file = iter.block.number_in_stream + stream_ref.block_number_base;
        iter.block.compressed_stream_offset = if record == 0 {
            group.node.compressed_base
        } else {
            vli_ceil4(group.records[record - 1].unpadded_sum)
        };
        iter.block.uncompressed_stream_offset = if record == 0 {
            group.node.uncompressed_base
        } else {
            group.records[record - 1].uncompressed_sum
        };
        iter.block.uncompressed_size =
            group.records[record].uncompressed_sum - iter.block.uncompressed_stream_offset;
        iter.block.unpadded_size =
            group.records[record].unpadded_sum - iter.block.compressed_stream_offset;
        iter.block.total_size = vli_ceil4(iter.block.unpadded_size);
        iter.block.compressed_stream_offset += LZMA_STREAM_HEADER_SIZE as u64;
        iter.block.compressed_file_offset =
            iter.block.compressed_stream_offset + iter.stream.compressed_offset;
        iter.block.uncompressed_file_offset =
            iter.block.uncompressed_stream_offset + iter.stream.uncompressed_offset;
    }
}

/// 初始化 LzmaIndexIter 结构体
pub fn lzma_index_iter_init(iter: &mut LzmaIndexIter, i: Box<LzmaIndex>) {
    iter.internal.set(ITER_INDEX, Internal::Index(i));
    lzma_index_iter_rewind(iter);
}

/// 重置 LzmaIndexIter 结构体
pub fn lzma_index_iter_rewind(iter: &mut LzmaIndexIter) {
    iter.internal.set(ITER_STREAM, Internal::None);
    iter.internal.set(ITER_GROUP, Internal::None);
    iter.internal.set(ITER_RECORD, Internal::Size(0));
    iter.internal
        .set(ITER_METHOD, Internal::Size(ITER_METHOD_NORMAL));
}

pub fn lzma_index_iter_next(iter: &mut LzmaIndexIter, mode: LzmaIndexIterMode) -> bool {
    use crate::api::Internal;
    use crate::common::{IndexGroup, IndexNode, IndexStream};
    use std::rc::Rc;

    // 捕获不支持的模式值
    if (mode.clone() as u32) > LzmaIndexIterMode::NonEmptyBlock as u32 {
        return true;
    }

    // 取出 index
    let i = match iter.internal.get(ITER_INDEX).unwrap().clone() {
        Internal::Index(i) => i,
        _ => return false,
    };
    // 取出 stream
    let mut stream = match iter.internal.get(ITER_STREAM).unwrap().clone() {
        Internal::Stream(s) => Some(s),
        _ => None,
    };
    let mut group: Option<Box<IndexGroup>> = None;
    let mut record: usize = match iter.internal.get(ITER_RECORD).unwrap().clone() {
        Internal::Size(r) => r,
        _ => 0,
    };

    // 如果请求下一个 Stream，将 group 设为 None
    if mode != LzmaIndexIterMode::Stream {
        // 获取当前组的指针
        let method = match iter.internal.get(ITER_METHOD).unwrap().clone() {
            Internal::Size(m) => m,
            _ => 0,
        };
        match method {
            ITER_METHOD_NORMAL => {
                group = match iter.internal.get(ITER_GROUP).unwrap().clone() {
                    Internal::Group(g) => Some(g),
                    _ => None,
                };
            }
            ITER_METHOD_NEXT => {
                group = match iter.internal.get(ITER_GROUP).unwrap().clone() {
                    Internal::Group(g) => {
                        // 找到下一个 group
                        let node_arc = Arc::new(Mutex::new(IndexNode::Group(
                            *(group.as_ref().unwrap()).clone(),
                        )));
                        let next = index_tree_next(&node_arc);
                        next.and_then(|arc| {
                            arc.lock().unwrap().as_group().map(|g| Box::new(g.clone()))
                        })
                    }
                    _ => None,
                };
            }
            ITER_METHOD_LEFTMOST => {
                if let Some(ref s) = stream {
                    if let Some(leftmost_arc) = s.groups.leftmost.as_ref() {
                        let leftmost_borrow = leftmost_arc.lock().unwrap();
                        if let Some(g) = leftmost_borrow.as_group() {
                            group = Some(Box::new(g.clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // again: 循环
    'again: loop {
        if stream.is_none() {
            // 在 lzma_index 的开头，定位第一个 Stream
            if let Some(leftmost_arc) = i.streams.leftmost.as_ref() {
                let leftmost_borrow = leftmost_arc.lock().unwrap();
                if let Some(s) = leftmost_borrow.as_stream() {
                    stream = Some(Box::new(s.clone()));
                }
            }
            if mode >= LzmaIndexIterMode::Block {
                // 跳过没有 Block 的 Stream
                while let Some(ref s) = stream {
                    if s.groups.leftmost.is_none() {
                        // 找下一个 stream
                        let node_arc = Arc::new(Mutex::new(IndexNode::Stream((**s).clone())));
                        let next = index_tree_next(&node_arc);
                        if let Some(next_arc) = next {
                            let next_borrow = next_arc.lock().unwrap();
                            if let Some(s2) = next_borrow.as_stream() {
                                stream = Some(Box::new(s2.clone()));
                                continue;
                            } else {
                                stream = None;
                                break;
                            }
                        } else {
                            stream = None;
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            // 从 Stream 中的第一个 Record 开始
            if let Some(ref s) = stream {
                if let Some(leftmost_arc) = s.groups.leftmost.as_ref() {
                    let leftmost_borrow = leftmost_arc.lock().unwrap();
                    if let Some(g) = leftmost_borrow.as_group() {
                        group = Some(Box::new(g.clone()));
                    }
                }
            }
            record = 0;
        } else if group.is_some() && record < group.as_ref().unwrap().last {
            // 下一个 Record 在同一组中
            record += 1;
        } else {
            // 该组没有更多的 Record 或者该 Stream 没有 Block
            record = 0;
            // 如果 group 不为 None，该 Stream 至少有一个 Block
            if group.is_some() {
                let node_arc = Arc::new(Mutex::new(IndexNode::Group(
                    *(group.as_ref().unwrap()).clone(),
                )));
                let next = index_tree_next(&node_arc);
                group = next
                    .and_then(|arc| arc.lock().unwrap().as_group().map(|g| Box::new(g.clone())));
            }
            if group.is_none() {
                // 该 Stream 没有更多的 Record，寻找下一个 Stream
                loop {
                    let s = stream.take().unwrap();
                    let node_arc = Arc::new(Mutex::new(IndexNode::Stream((*s).clone())));
                    let next = index_tree_next(&node_arc);
                    if next.is_none() {
                        return true;
                    }
                    if let Some(next_arc) = next {
                        let next_borrow = next_arc.lock().unwrap();
                        if let Some(s2) = next_borrow.as_stream() {
                            stream = Some(Box::new(s2.clone()));
                            if mode < LzmaIndexIterMode::Block || s2.groups.leftmost.is_some() {
                                break;
                            }
                        } else {
                            stream = None;
                            break;
                        }
                    } else {
                        stream = None;
                        break;
                    }
                }
                // 新 stream 的第一个 group
                if let Some(ref s) = stream {
                    if let Some(leftmost_arc) = s.groups.leftmost.as_ref() {
                        let leftmost_borrow = leftmost_arc.lock().unwrap();
                        if let Some(g) = leftmost_borrow.as_group() {
                            group = Some(Box::new(g.clone()));
                        }
                    }
                }
            }
        }

        // 非空块模式下，跳过空块
        if mode == LzmaIndexIterMode::NonEmptyBlock {
            if let Some(ref g) = group {
                if record == 0 {
                    if g.node.uncompressed_base == g.records[0].uncompressed_sum {
                        // goto again
                        // continue 'again;
                        // Rust: loop back
                        // continue 'again;
                        // Actually, just restart the loop
                        // (simulate goto again)
                        // But to avoid infinite loop, check for end
                        // If group is last, break
                        // But here, just continue
                        // (should be safe as per C logic)
                        continue 'again;
                    }
                } else if g.records[record - 1].uncompressed_sum
                    == g.records[record].uncompressed_sum
                {
                    continue 'again;
                }
            }
        }
        break;
    }

    // 更新 iter.internal
    if let Some(ref s) = stream {
        iter.internal
            .set(ITER_STREAM, Internal::Stream(s.clone()))
            .unwrap();
    } else {
        iter.internal.set(ITER_STREAM, Internal::None).unwrap();
    }
    if let Some(ref g) = group {
        iter.internal
            .set(ITER_GROUP, Internal::Group(g.clone()))
            .unwrap();
    } else {
        iter.internal.set(ITER_GROUP, Internal::None).unwrap();
    }
    iter.internal
        .set(ITER_RECORD, Internal::Size(record))
        .unwrap();
    iter_set_info(iter);
    false
}

pub fn lzma_index_iter_locate(iter: &mut LzmaIndexIter, target: LzmaVli) -> bool {
    use crate::api::Internal;
    use crate::common::{IndexGroup, IndexStream};
    // 取出 index
    let i = match iter.internal.get(ITER_INDEX).unwrap().clone() {
        Internal::Index(i) => i,
        _ => return false,
    };
    // 如果目标超出文件末尾，立即返回
    if i.uncompressed_size <= target {
        return true;
    }
    // 定位包含目标偏移量的 Stream
    let stream_arc = index_tree_locate(&i.streams, target).expect("stream not found");
    let stream_borrow = stream_arc.lock().unwrap();
    let stream = stream_borrow.as_stream().expect("not a stream");
    let mut target = target - stream.node.uncompressed_base;
    // 定位包含目标偏移量的 Group
    let group_arc = index_tree_locate(&stream.groups, target).expect("group not found");
    let group_borrow = group_arc.lock().unwrap();
    let group = group_borrow.as_group().expect("not a group");
    // 二分查找定位 Record
    let mut left = 0;
    let mut right = group.last;
    while left < right {
        let pos = left + (right - left) / 2;
        if group.records[pos].uncompressed_sum <= target {
            left = pos + 1;
        } else {
            right = pos;
        }
    }
    // 设置 iter.internal
    iter.internal
        .set(ITER_STREAM, Internal::Stream(Box::new(stream.clone())))
        .unwrap();
    iter.internal
        .set(ITER_GROUP, Internal::Group(Box::new(group.clone())))
        .unwrap();
    iter.internal
        .set(ITER_RECORD, Internal::Size(left))
        .unwrap();
    iter_set_info(iter);
    false
}
