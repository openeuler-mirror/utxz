/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::sync::{Arc, Mutex, Weak};

use crate::api::{LzmaStreamFlags, LzmaVli};

pub type IndexNodeRef = Arc<Mutex<IndexNode>>;

/// 节点类型枚举
#[derive(Debug, Clone, Copy)]
pub enum NodeType {
    Stream,
    Group,
}

#[derive(Debug)]
pub enum IndexNode {
    Stream(IndexStream),
    Group(IndexGroup),
}

impl IndexNode {
    /// 获取树节点信息
    pub fn get_tree_node(&self) -> &IndexTreeNode {
        match self {
            IndexNode::Stream(stream) => &stream.node,
            IndexNode::Group(group) => &group.node,
        }
    }

    /// 获取树节点信息（可变引用）
    pub fn get_tree_node_mut(&mut self) -> &mut IndexTreeNode {
        match self {
            IndexNode::Stream(stream) => &mut stream.node,
            IndexNode::Group(group) => &mut group.node,
        }
    }

    /// 获取未压缩基址
    pub fn uncompressed_base(&self) -> LzmaVli {
        self.get_tree_node().uncompressed_base
    }

    /// 获取压缩基址
    pub fn compressed_base(&self) -> LzmaVli {
        self.get_tree_node().compressed_base
    }

    /// 检查是否为 Stream
    pub fn is_stream(&self) -> bool {
        matches!(self, IndexNode::Stream(_))
    }

    /// 检查是否为 Group
    pub fn is_group(&self) -> bool {
        matches!(self, IndexNode::Group(_))
    }

    /// 尝试获取 Stream 引用
    pub fn as_stream(&self) -> Option<&IndexStream> {
        match self {
            IndexNode::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    /// 尝试获取 Group 引用
    pub fn as_group(&self) -> Option<&IndexGroup> {
        match self {
            IndexNode::Group(group) => Some(group),
            _ => None,
        }
    }

    /// 尝试获取 Stream 可变引用
    pub fn as_stream_mut(&mut self) -> Option<&mut IndexStream> {
        match self {
            IndexNode::Stream(stream) => Some(stream),
            _ => None,
        }
    }

    /// 尝试获取 Group 可变引用
    pub fn as_group_mut(&mut self) -> Option<&mut IndexGroup> {
        match self {
            IndexNode::Group(group) => Some(group),
            _ => None,
        }
    }
}

/// Base structure for index_stream and index_group structures
#[derive(Debug)]
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

impl IndexTreeNode {
    pub fn new(uncompressed_base: LzmaVli, compressed_base: LzmaVli) -> Self {
        Self {
            uncompressed_base,
            compressed_base,
            parent: None,
            left: None,
            right: None,
        }
    }

    /// Get parent node
    pub fn get_parent(&self) -> Option<Arc<Mutex<IndexNode>>> {
        self.parent.as_ref().and_then(|weak| weak.upgrade())
    }

    /// Get left child
    pub fn get_left(&self) -> Option<Arc<Mutex<IndexNode>>> {
        self.left.clone() //增加引用计数
    }

    /// Get right child
    pub fn get_right(&self) -> Option<Arc<Mutex<IndexNode>>> {
        self.right.clone()
    }

    /// Set parent node
    pub fn set_parent(&mut self, parent: Option<Arc<Mutex<IndexNode>>>) {
        self.parent = parent.map(|arc| Arc::downgrade(&arc));
    }

    /// Set left child
    pub fn set_left(&mut self, left: Option<Arc<Mutex<IndexNode>>>) {
        self.left = left;
    }

    /// Set right child
    pub fn set_right(&mut self, right: Option<Arc<Mutex<IndexNode>>>) {
        self.right = right;
    }
}

impl Clone for IndexTreeNode {
    fn clone(&self) -> Self {
        IndexTreeNode {
            uncompressed_base: self.uncompressed_base,
            compressed_base: self.compressed_base,
            parent: self.parent.clone(),
            left: self.left.clone(),
            right: self.right.clone(),
        }
    }
}

/// AVL tree to hold index_stream or index_group structures
#[derive(Debug)]
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

impl IndexTree {
    pub fn new() -> Self {
        Self {
            root: None,
            leftmost: None,
            rightmost: None,
            count: 0,
        }
    }

    /// Create a new node
    pub fn create_node(
        &self,
        uncompressed_base: LzmaVli,
        compressed_base: LzmaVli,
        node_type: NodeType,
    ) -> Arc<Mutex<IndexNode>> {
        match node_type {
            NodeType::Stream => {
                let stream = IndexStream {
                    node: IndexTreeNode::new(uncompressed_base, compressed_base),
                    number: 0,
                    block_number_base: 0,
                    groups: IndexTree::new(),
                    record_count: 0,
                    index_list_size: 0,
                    stream_flags: LzmaStreamFlags::default(),
                    stream_padding: 0,
                };
                Arc::new(Mutex::new(IndexNode::Stream(stream)))
            }
            NodeType::Group => {
                let group = IndexGroup {
                    node: IndexTreeNode::new(uncompressed_base, compressed_base),
                    number_base: 0,
                    allocated: 0,
                    last: 0,
                    records: Vec::new(),
                };
                Arc::new(Mutex::new(IndexNode::Group(group)))
            }
        }
    }
    pub fn set_root(&mut self, node: Option<Arc<Mutex<IndexNode>>>) {
        self.root = node;
    }

    pub fn set_rightmost(&mut self, node: Option<Arc<Mutex<IndexNode>>>) {
        self.rightmost = node;
    }

    pub fn set_leftmost(&mut self, node: Option<Arc<Mutex<IndexNode>>>) {
        self.leftmost = node;
    }

    // /// Add a new node to the tree (sequential insertion)
    // pub fn add_node(&mut self, uncompressed_base: LzmaVli, compressed_base: LzmaVli, node_type: NodeType) -> Arc<Mutex<IndexNode>> {
    //     let new_node = self.create_node(uncompressed_base, compressed_base, node_type);

    //     if self.root.is_none() {
    //         // First node becomes root, leftmost, and rightmost
    //         self.root = Some(new_node.clone());
    //         self.leftmost = Some(new_node.clone());
    //         self.rightmost = Some(new_node.clone());
    //     } else {
    //         // Add to the right of rightmost node (sequential insertion)
    //         if let Some(ref rightmost) = self.rightmost {
    //             {
    //                 let mut rightmost_mut = rightmost.lock().unwrap();
    //                 let tree_node = rightmost_mut.get_tree_node_mut();
    //                 tree_node.set_right(new_node.clone());
    //             }
    //             {
    //                 let mut new_node_mut = new_node.lock().unwrap();
    //                 let tree_node = new_node_mut.get_tree_node_mut();
    //                 tree_node.set_parent(Arc::downgrade(rightmost));
    //             }
    //         }
    //         self.rightmost = Some(new_node.clone());
    //     }

    //     self.count += 1;
    //     new_node
    // }

    // /// Insert node at specific position (for AVL tree balancing)
    // pub fn insert_node(&mut self, uncompressed_base: LzmaVli, compressed_base: LzmaVli, node_type: NodeType) -> Arc<Mutex<IndexNode>> {
    //     let new_node = self.create_node(uncompressed_base, compressed_base, node_type);

    //     if self.root.is_none() {
    //         self.root = Some(new_node.clone());
    //         self.leftmost = Some(new_node.clone());
    //         self.rightmost = Some(new_node.clone());
    //     } else {
    //         // Find insertion position based on uncompressed_base
    //         let mut current = self.root.clone();
    //         let mut parent = None;

    //         while let Some(ref current_node) = current {
    //             parent = current.clone();

    //             let next = {
    //                 let current_borrow = current_node.lock().unwrap();

    //                 if uncompressed_base < current_borrow.uncompressed_base {
    //                     current_borrow.left.clone()
    //                 } else {
    //                     current_borrow.right.clone()
    //                 }
    //             };
    //             current = next;
    //         }

    //         // Insert the new node
    //         if let Some(ref parent_node) = parent {
    //             let mut parent_mut = parent_node.lock().unwrap();
    //             let mut new_node_mut = new_node.lock().unwrap();

    //             if uncompressed_base < parent_mut.uncompressed_base {
    //                 parent_mut.set_left(new_node.clone());
    //             } else {
    //                 parent_mut.set_right(new_node.clone());
    //             }
    //             new_node_mut.set_parent(Arc::downgrade(parent_node));

    //             // Update leftmost/rightmost if necessary
    //             if let Some(ref leftmost) = self.leftmost {

    //                 let need_update_leftmost = {
    //                     let leftmost_borrow = leftmost.lock().unwrap();
    //                     uncompressed_base < leftmost_borrow.uncompressed_base
    //                 };
    //                 if need_update_leftmost {
    //                     self.leftmost = Some(new_node.clone());
    //                 }
    //             }

    //             if let Some(ref rightmost) = self.rightmost {
    //                 let need_update_rightmost = {
    //                     let rightmost_borrow = rightmost.lock().unwrap();
    //                     uncompressed_base > rightmost_borrow.uncompressed_base
    //                 };
    //                 if need_update_rightmost {
    //                     self.rightmost = Some(new_node.clone());
    //                 }
    //             }
    //         }
    //     }

    //     self.count += 1;
    //     new_node
    // }

    // Find node by uncompressed_base
    // pub fn find_node(&self, uncompressed_base: LzmaVli) -> Option<Arc<Mutex<IndexTreeNode>>> {
    //     let mut current = self.root.clone();

    //     while let Some(ref current_node) = current {
    //         let next = {
    //             let current_borrow = current_node.lock().unwrap();
    //             if uncompressed_base < current_borrow.uncompressed_base {
    //                 current_borrow.left.clone()
    //             } else {
    //                 current_borrow.right.clone()
    //             }
    //         };
    //         current = next;
    //     }

    //     None
    // }

    /// Get root node
    // pub fn get_root(&self) -> Option<Arc<Mutex<IndexTreeNode>>> {
    //     self.root.clone()
    // }

    // /// Get leftmost node
    // pub fn get_leftmost(&self) -> Option<Arc<Mutex<IndexTreeNode>>> {
    //     self.leftmost.clone()
    // }

    // /// Get rightmost node
    // pub fn get_rightmost(&self) -> Option<Arc<Mutex<IndexTreeNode>>> {
    //     self.rightmost.clone()
    //}

    /// Check if tree is empty
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Get tree size
    pub fn size(&self) -> u32 {
        self.count
    }

    /// Clear the tree
    pub fn clear(&mut self) {
        self.root = None;
        self.leftmost = None;
        self.rightmost = None;
        self.count = 0;
    }

    /// In-order traversal
    pub fn inorder_traversal<F>(&self, mut visitor: F)
    where
        F: FnMut(&IndexNode),
    {
        if let Some(ref root) = self.root {
            self.inorder_recursive(Some(root.clone()), &mut visitor);
        }
    }

    fn inorder_recursive<F>(&self, node: Option<Arc<Mutex<IndexNode>>>, visitor: &mut F)
    where
        F: FnMut(&IndexNode),
    {
        if let Some(ref node_arc) = node {
            let node_borrow = node_arc.lock().unwrap();
            let tree_node = node_borrow.get_tree_node();

            // Traverse left subtree
            if let Some(ref left) = tree_node.left {
                self.inorder_recursive(Some(left.clone()), visitor);
            }

            // Visit current node
            visitor(&*node_borrow);

            // Traverse right subtree
            if let Some(ref right) = tree_node.right {
                self.inorder_recursive(Some(right.clone()), visitor);
            }
        }
    }

    /// Print tree structure (for debugging)
    pub fn print_tree(&self) {
        println!("=== Tree Structure ===");
        println!("Size: {}", self.size());
        println!(
            "Root: {:?}",
            self.root
                .as_ref()
                .map(|r| r.lock().unwrap().uncompressed_base())
        );
        println!(
            "Leftmost: {:?}",
            self.leftmost
                .as_ref()
                .map(|l| l.lock().unwrap().uncompressed_base())
        );
        println!(
            "Rightmost: {:?}",
            self.rightmost
                .as_ref()
                .map(|r| r.lock().unwrap().uncompressed_base())
        );

        if let Some(ref root) = self.root {
            self.print_node_recursive(root.lock().unwrap().get_tree_node(), 0);
        }
    }

    fn print_node_recursive(&self, node: &IndexTreeNode, depth: usize) {
        let indent = "  ".repeat(depth);

        println!(
            "{}Node: uncompressed={}, compressed={}",
            indent, node.uncompressed_base, node.compressed_base
        );

        if let Some(ref left) = node.left {
            println!("{}Left:", indent);
            self.print_node_recursive(left.lock().unwrap().get_tree_node(), depth + 1);
        }

        if let Some(ref right) = node.right {
            println!("{}Right:", indent);
            self.print_node_recursive(right.lock().unwrap().get_tree_node(), depth + 1);
        }
    }
}

impl Default for IndexTree {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for IndexTree {
    fn clone(&self) -> Self {
        IndexTree {
            root: self.root.clone(),
            leftmost: self.leftmost.clone(),
            rightmost: self.rightmost.clone(),
            count: self.count,
        }
    }
}

/// 存储单个记录的未压缩和未填充的累积大小
#[derive(Debug, Clone)]
pub struct IndexRecord {
    pub uncompressed_sum: LzmaVli,
    pub unpadded_sum: LzmaVli,
}

/// 记录组是 index_stream.groups 树的一部分
#[derive(Debug, Clone)]
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

impl IndexGroup {
    pub fn new(size: usize) -> Self {
        IndexGroup {
            node: IndexTreeNode::new(0, 0),
            number_base: 0,
            allocated: 0,
            last: 0,
            records: Vec::with_capacity(size), // 默认空的 Vec
        }
    }
}

/// 每个 index_stream 都是 Streams 树中的一个节点。
#[derive(Debug, Clone)]
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

impl Default for IndexStream {
    fn default() -> Self {
        IndexStream {
            node: IndexTreeNode::new(0, 0),
            number: 0,
            block_number_base: 0,
            groups: IndexTree::default(),
            record_count: 0,
            index_list_size: 0,
            stream_flags: LzmaStreamFlags::default(),
            stream_padding: 0,
        }
    }
}
#[derive(Debug, Default, Clone)]

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

const INDEX_GROUP_SIZE: usize = 512; // 每个组的默认记录数量

impl LzmaIndex {
    /// 创建新的 LzmaIndex
    pub fn new() -> Self {
        Self {
            streams: IndexTree::new(),
            uncompressed_size: 0,
            total_size: 0,
            record_count: 0,
            index_list_size: 0,
            prealloc: INDEX_GROUP_SIZE,
            checks: 0,
        }
    }

    // /// 添加新的流到索引中
    // pub fn add_stream(&mut self, stream: IndexStream) -> Arc<Mutex<IndexNode>> {
    //     // 创建 IndexNode::Stream 并添加到树中
    //     let stream_node = Arc::new(Mutex::new(IndexNode::Stream(stream)));

    //     if self.streams.root.is_none() {
    //         self.streams.root = Some(stream_node.clone());
    //         self.streams.leftmost = Some(stream_node.clone());
    //         self.streams.rightmost = Some(stream_node.clone());
    //     } else {
    //         // 添加到最右节点
    //         if let Some(ref rightmost) = self.streams.rightmost {
    //             {
    //                 let mut rightmost_mut = rightmost.lock().unwrap();
    //                 let tree_node = rightmost_mut.get_tree_node_mut();
    //                 tree_node.set_right(stream_node.clone());
    //             }
    //             {
    //                 let mut stream_node_mut = stream_node.lock().unwrap();
    //                 let tree_node = stream_node_mut.get_tree_node_mut();
    //                 tree_node.set_parent(Arc::downgrade(rightmost));
    //             }
    //         }
    //         self.streams.rightmost = Some(stream_node.clone());
    //     }

    //     self.streams.count += 1;
    //     stream_node
    // }

    /// 获取流的数量
    pub fn stream_count(&self) -> u32 {
        // 直接访问，无需 .lock()
        self.streams.size()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        // 直接访问，无需 .lock()
        self.streams.is_empty()
    }

    /// 遍历所有流
    pub fn for_each_stream<F>(&self, mut visitor: F)
    where
        F: FnMut(&IndexNode),
    {
        // 直接访问，无需 .lock()
        self.streams.inorder_traversal(visitor);
    }

    /// 遍历所有 Stream 节点
    pub fn for_each_stream_only<F>(&self, mut visitor: F)
    where
        F: FnMut(&IndexStream),
    {
        self.streams.inorder_traversal(|node| {
            if let Some(stream) = node.as_stream() {
                visitor(stream);
            }
        });
    }

    /// 遍历所有 Group 节点
    pub fn for_each_group_only<F>(&self, mut visitor: F)
    where
        F: FnMut(&IndexGroup),
    {
        self.streams.inorder_traversal(|node| {
            if let Some(group) = node.as_group() {
                visitor(group);
            }
        });
    }

    /// 清空所有流
    pub fn clear_streams(&mut self) {
        // 直接访问，无需 .lock()
        self.streams.clear();
        self.uncompressed_size = 0;
        self.total_size = 0;
        self.record_count = 0;
        self.index_list_size = 0;
    }
}
