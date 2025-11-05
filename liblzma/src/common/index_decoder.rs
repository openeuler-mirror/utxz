/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::LzmaVli;

use super::LzmaIndex;

use std::sync::{Arc, Mutex};

/// 用于表示 LZMA 索引解码器的结构体
#[derive(Debug)]
pub struct LzmaIndexDecoder {
    /// 解码序列状态
    pub sequence: Sequence,

    /// 内存使用限制
    pub memlimit: u64,

    /// 目标索引
    pub index: Option<Box<LzmaIndex>>,

    /// 应用程序提供的指针，在成功解码后设置
    pub index_ptr: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,

    /// 剩余待解码的记录数
    pub count: LzmaVli,

    /// 最近的未填充大小字段
    pub unpadded_size: LzmaVli,

    /// 最近的未压缩大小字段
    pub uncompressed_size: LzmaVli,

    /// 整数中的位置
    pub pos: usize,

    /// 记录列表字段的 CRC32
    pub crc32: u32,
}

impl Clone for LzmaIndexDecoder {
    fn clone(&self) -> Self {
        LzmaIndexDecoder {
            sequence: self.sequence,
            memlimit: self.memlimit,
            // 安全方式：克隆时将 mutable 引用置空
            index: None,
            index_ptr: None,
            count: self.count,
            unpadded_size: self.unpadded_size,
            uncompressed_size: self.uncompressed_size,
            pos: self.pos,
            crc32: self.crc32,
        }
    }
}

impl Default for LzmaIndexDecoder {
    fn default() -> Self {
        LzmaIndexDecoder {
            sequence: Sequence::Indicator, // 假设 `Sequence` 实现了 `Default` 或者默认是 `SeqInit`
            memlimit: 0,
            index: None,          // `Option<&'a mut LzmaIndex>` 默认是 None
            index_ptr: None,      // `Option<&'a mut LzmaIndex>` 默认是 None
            count: 0,             // 假设 `LzmaVli` 是某种类型，可以初始化为 0
            unpadded_size: 0,     // 假设 `LzmaVli` 可以初始化为 0
            uncompressed_size: 0, // 假设 `LzmaVli` 可以初始化为 0
            pos: 0,
            crc32: 0,
        }
    }
}
/// 解码序列的枚举，表示解码过程中的不同阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sequence {
    Indicator,
    Count,
    MemUsage,
    Unpadded,
    Uncompressed,
    PaddingInit,
    Padding,
    Crc32,
}
