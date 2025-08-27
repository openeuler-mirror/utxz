/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(clippy::new_without_default)]
use crate::common::{IndexGroup, IndexStream, IndexTreeNode, LzmaIndex};

use super::{LzmaStreamFlags, LzmaVli};

#[derive(Debug)]
pub struct LzmaIndexIter {
    pub stream: Stream,
    pub block: Block,
    // pub internal: [Internal; 6],
    pub internal: InternalArray,
}

impl Clone for LzmaIndexIter {
    fn clone(&self) -> Self {
        LzmaIndexIter {
            stream: self.stream.clone(),
            block: self.block.clone(),
            internal: self.internal.clone(),
        }
    }
}

impl Default for LzmaIndexIter {
    fn default() -> Self {
        LzmaIndexIter {
            stream: Stream::default(), // 假设 Stream 实现了 Default
            block: Block::default(),   // 假设 Block 实现了 Default
            // internal: [Internal::default(); 6], // 假设 Internal 实现了 Default
            internal: InternalArray::new(),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Stream {
    pub flags: Option<LzmaStreamFlags>,
    // pub reserved_ptr1: Option<T>,
    // pub reserved_ptr2: Option<T>,
    // pub reserved_ptr3: Option<T>,
    pub number: LzmaVli,
    pub block_count: LzmaVli,
    pub compressed_offset: LzmaVli,
    pub uncompressed_offset: LzmaVli,
    pub compressed_size: LzmaVli,
    pub uncompressed_size: LzmaVli,
    pub padding: LzmaVli,
    pub reserved_vli1: LzmaVli,
    pub reserved_vli2: LzmaVli,
    pub reserved_vli3: LzmaVli,
    pub reserved_vli4: LzmaVli,
}

#[derive(Default, Debug, Clone)]
pub struct Block {
    pub number_in_file: LzmaVli,
    pub compressed_file_offset: LzmaVli,
    pub uncompressed_file_offset: LzmaVli,
    pub number_in_stream: LzmaVli,
    pub compressed_stream_offset: LzmaVli,
    pub uncompressed_stream_offset: LzmaVli,
    pub uncompressed_size: LzmaVli,
    pub unpadded_size: LzmaVli,
    pub total_size: LzmaVli,
    pub reserved_vli1: LzmaVli,
    pub reserved_vli2: LzmaVli,
    pub reserved_vli3: LzmaVli,
    pub reserved_vli4: LzmaVli,
    // pub reserved_ptr1: Option<T>,
    // pub reserved_ptr2: Option<T>,
    // pub reserved_ptr3: Option<T>,
    // pub reserved_ptr4: Option<T>,
}
#[derive(Clone, Default, Debug)]
pub enum Internal {
    // 使用引用代替指针
    Index(Box<LzmaIndex>),
    Stream(Box<IndexStream>),
    Group(Box<IndexGroup>),

    // 值类型保持不变
    Size(usize),
    Vli(LzmaVli),

    // 表示空值
    #[default]
    None,
}
impl Internal {
    // 使用引用模式匹配
    pub fn as_index(&self) -> Option<&LzmaIndex> {
        match self {
            Internal::Index(index) => Some(index),
            _ => None,
        }
    }

    pub fn as_stream(&self) -> Option<Box<IndexStream>> {
        match self {
            Internal::Stream(stream) => Some(stream.clone()),
            _ => None,
        }
    }

    pub fn as_group(&self) -> Option<&IndexGroup> {
        match self {
            Internal::Group(group) => Some(group),
            _ => None,
        }
    }

    pub fn as_size(&self) -> Option<usize> {
        match self {
            Internal::Size(val) => Some(*val),
            _ => None,
        }
    }

    pub fn as_vli(&self) -> Option<LzmaVli> {
        match self {
            Internal::Vli(val) => Some(*val),
            _ => None,
        }
    }
}

// 封装数组
#[derive(Clone, Debug)]
pub struct InternalArray {
    data: Vec<Internal>,
}

impl InternalArray {
    pub fn new() -> Self {
        Self {
            data: vec![Internal::None; 6],
        }
    }

    // 修改为 &self，内部借用可变引用
    pub fn set(&mut self, index: usize, value: Internal) -> Result<(), &str> {
        if index >= 6 {
            return Err("Index out of bounds"); // 返回 &'a str 类型
        }
        self.data[index] = value;
        Ok(())
    }

    pub fn set_tree_node(&mut self, index: usize, node: IndexTreeNode) -> Result<(), &str> {
        if index >= 6 {
            return Err("Index out of bounds"); // 返回 &'a str 类型
        }
        // 利用 From trait 将 IndexTreeNode 转换为 IndexGroup
        //let group = IndexGroup::from(node);
        let group = IndexGroup {
            node: node.clone(),
            number_base: 0,
            allocated: 0,
            last: 0,
            records: Vec::new(),
        };
        // 把 group 放入 Box 中，并泄漏来获得一个 &'static IndexGroup
        let leaked_group: &'static IndexGroup = Box::leak(Box::new(group));
        self.data[index] = Internal::Group(Box::new(leaked_group.clone()));
        Ok(())
    }

    // 返回 Ref<T> 类型以便在内部静态借用
    pub fn get(&self, index: usize) -> Option<&Internal> {
        if index >= 6 {
            None
        } else {
            // map 将整个 borrow 转换为对数组中某一项的借用
            Some(&self.data[index])
        }
    }
}
#[derive(PartialEq, PartialOrd, Clone)]
pub enum LzmaIndexIterMode {
    Any = 0,
    Stream = 1,
    Block = 2,
    NonEmptyBlock = 3,
}
