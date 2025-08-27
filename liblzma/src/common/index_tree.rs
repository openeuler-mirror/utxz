/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaStreamFlags, LzmaVli};
use std::sync::{Arc, Mutex, Weak};

/// AVL tree to hold index_stream or index_group structures
#[derive(Clone, Debug)]
pub struct IndexTree {
    /// Root node
    pub root: Option<Arc<Mutex<IndexNode>>>,

    /// Leftmost node. Since the tree will be filled sequentially,
    /// this won't change after the first node has been added to
    /// the tree.
    pub leftmost: Option<Arc<Mutex<IndexNode>>>,

    /// The rightmost node in the tree. Since the tree is filled
    /// sequentially, this is always the node where to add the new data.
    pub rightmost: Option<Arc<Mutex<IndexNode>>>,

    /// Number of nodes in the tree
    pub count: u32,
}

/// 每个 index_stream 都是 Streams 树中的一个节点。
#[derive(Clone, Debug)]
pub struct IndexStream {
    /// 作为树节点
    pub node: IndexTreeNode,
    /// 该流的编号（第一个为 1）
    pub number: u32,
    /// 该流之前的所有块总数
    pub block_number_base: LzmaVli,
    /// 该流的记录组，以树结构存储（T-tree + AVL 平衡）
    /// 默认每个节点有 INDEX_GROUP_SIZE 条记录，便于内存分配和查找
    pub groups: IndexTree,
    /// 该流中的记录数
    pub record_count: LzmaVli,
    /// 该流的记录列表字段大小，用于计算 Index 字段和流总大小
    pub index_list_size: LzmaVli,
    /// 该流的流标志（stream_flags.version == UINT32_MAX 表示未知）
    pub stream_flags: LzmaStreamFlags,
    /// 该流之后的填充量，默认为 0
    pub stream_padding: LzmaVli,
}
#[derive(Debug)]
pub enum IndexNode {
    Stream(IndexStream),
    Group(IndexGroup),
}
/// Base structure for index_stream and index_group structures
#[derive(Clone, Debug)]
pub struct IndexTreeNode {
    /// Uncompressed start offset of this Stream (relative to the
    /// beginning of the file) or Block (relative to the beginning
    /// of the Stream)
    pub uncompressed_base: LzmaVli,

    /// Compressed start offset of this Stream or Block
    pub compressed_base: LzmaVli,

    pub parent: Option<Weak<Mutex<IndexNode>>>,
    pub left: Option<Arc<Mutex<IndexNode>>>,
    pub right: Option<Arc<Mutex<IndexNode>>>,
}

/// 存储单个记录的未压缩和未填充的累积大小
#[derive(Debug, Clone)]
pub struct IndexRecord {
    pub uncompressed_sum: LzmaVli,
    pub unpadded_sum: LzmaVli,
}

/// 记录组是 index_stream.groups 树的一部分
#[derive(Clone, Debug)]
pub struct IndexGroup {
    /// 作为 AVL 树的一部分
    pub node: IndexTreeNode,

    /// 该组前的块数
    pub number_base: LzmaVli,

    /// 可存储的记录数量
    pub allocated: usize,

    /// 最后一个使用的记录索引
    pub last: usize,

    /// 这些大小存储为累积和，以便在 lzma_index_locate() 中使用二分查找。
    ///
    /// 注意：unpadded_sum 的累加是特殊处理的：前一个值会先向上取整到4的倍数，再加上新块的 Unpadded Size。
    /// 例如，未填充大小为 39、57、81 时，存储的值为 39, 97 (40 + 57), 181 (100 + 81)。
    /// 这些块的总编码大小为 184。
    ///
    /// 这是一个灵活数组，便于优化内存使用。
    pub records: Vec<IndexRecord>,
}

#[derive(Clone, Debug)]

pub struct LzmaIndex {
    /// `IndexStream` 组成的 AVL 树
    ///
    pub streams: IndexTree, //stream 树的根

    /// 所有流的未压缩总大小
    pub uncompressed_size: LzmaVli,

    /// 所有流的总大小（包括压缩和元数据）
    pub total_size: LzmaVli,

    /// 所有流中的总记录数
    pub record_count: LzmaVli,

    /// 记录列表的大小（假设所有流被合并为一个流）
    pub index_list_size: LzmaVli,

    /// `lzma_index_append()` 预分配的记录数量（默认为 `INDEX_GROUP_SIZE`）
    pub prealloc: usize,

    /// 记录使用的完整性检查类型（不包括最后一个流）
    pub checks: u32,
}
