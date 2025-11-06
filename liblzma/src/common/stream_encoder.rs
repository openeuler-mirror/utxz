/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use num_enum::TryFromPrimitive;

use crate::{
    api::{
         LzmaBlock,  LzmaFilter, 
        LZMA_BLOCK_HEADER_SIZE_MAX, LZMA_FILTERS_MAX, 
    },

};

use super::{
    LzmaIndex, LzmaNextCoder,
};

#[repr(C)]
#[derive(Debug)]
pub struct LzmaStreamEncoder {
    /// 编码序列状态
    sequence: StreamSequence,

    /// 如果通过 stream_encoder_init() 或 stream_encoder_update()
    /// 已经初始化了块编码器，则为 true。这种情况下，
    /// 在 stream_encode() 中就不需要再次初始化。
    block_encoder_is_initialized: bool,

    /// 块编码器
    block_encoder: Box<LzmaNextCoder>,

    /// 块编码器的配置选项
    block_options: LzmaBlock,

    /// 当前使用的过滤器链
    filters: [LzmaFilter; LZMA_FILTERS_MAX + 1],

    /// 索引编码器。这个与块编码器是分开的，因为它不占用太多内存，
    /// 而且当使用相同的编码选项编码多个流时，我们可以避免重新分配内存。
    index_encoder: Box<LzmaNextCoder>,

    /// 用于保存块大小的索引
    index: Option<Box<LzmaIndex>>,

    /// 缓冲区的读取位置
    buffer_pos: usize,

    /// 缓冲区中的总字节数
    buffer_size: usize,
    /// 用于保存流头部、块头部和流尾部的缓冲区。
    /// 块头部具有最大的最大尺寸。
    buffer: [u8; LZMA_BLOCK_HEADER_SIZE_MAX as usize],
}

impl Clone for LzmaStreamEncoder {
    fn clone(&self) -> Self {
        LzmaStreamEncoder {
            sequence: self.sequence,
            block_encoder_is_initialized: self.block_encoder_is_initialized,
            block_encoder: Box::new(LzmaNextCoder::default()), // 创建新的默认实例
            block_options: self.block_options.clone(),
            filters: self.filters.clone(),
            index_encoder: Box::new(LzmaNextCoder::default()), // 创建新的默认实例
            index: self.index.clone(),
            buffer_pos: self.buffer_pos,
            buffer_size: self.buffer_size,
            buffer: self.buffer,
        }
    }
}

impl Default for LzmaStreamEncoder {
    fn default() -> Self {
        LzmaStreamEncoder {
            sequence: StreamSequence::default(), // 假设 StreamSequence 实现了 Default
            block_encoder_is_initialized: false,
            block_encoder: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            block_options: LzmaBlock::default(),               // 假设 LzmaBlock 实现了 Default
            filters: core::array::from_fn(|_| LzmaFilter::default()),
            index_encoder: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            index: None,                                       // Option 类型默认是 None
            buffer_pos: 0,
            buffer_size: 0,
            buffer: [0; LZMA_BLOCK_HEADER_SIZE_MAX as usize], // 初始化为全零数组
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, TryFromPrimitive, Default)]
enum StreamSequence {
    /// 流头部
    #[default]
    StreamHeader,
    /// 块初始化
    BlockInit,
    /// 块头部
    BlockHeader,
    /// 块编码
    BlockEncode,
    /// 索引编码
    IndexEncode,
    /// 流尾部
    StreamFooter,
}
