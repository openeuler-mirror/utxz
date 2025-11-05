/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::{
    api::{
         LzmaBlock, LzmaStreamFlags,
        LZMA_BLOCK_HEADER_SIZE_MAX, 
    },
    
};
use super::{
     LzmaIndexHash, LzmaNextCoder,
    
};

/// LZMA 流解码器结构体
#[derive(Debug)]
pub struct LzmaStreamDecoder {
    /// 解码序列状态，表示当前解码器所处的阶段
    sequence: Sequence,

    /// 块解码器，用于实际的数据解压缩
    block_decoder: Box<LzmaNextCoder>,

    /// 块选项，由块头部解码器解码，供块解码器使用
    block_options: LzmaBlock,

    /// 从流头部获取的流标志
    stream_flags: LzmaStreamFlags,

    /// 索引哈希，用于以 O(1) 的内存使用量比较块的大小
    /// 使用 Option 来表示可能为空的指针
    index_hash: Option<Box<LzmaIndexHash>>,

    /// 内存使用限制（以字节为单位）
    memlimit: u64,

    /// 实际需要的内存估计值（以字节为单位）
    memusage: u64,

    /// 如果为 true，当流没有完整性检查时返回 LZMA_NO_CHECK
    tell_no_check: bool,

    /// 如果为 true，当流使用了当前 liblzma 版本不支持的完整性检查时
    /// 返回 LZMA_UNSUPPORTED_CHECK
    tell_unsupported_check: bool,

    /// 如果为 true，在解码流头部后返回 LZMA_GET_CHECK
    tell_any_check: bool,

    /// 如果为 true，将告诉块解码器跳过计算和验证完整性检查
    ignore_check: bool,

    /// 如果为 true，将解码可能在中间或结尾包含流填充的连接流
    /// 当应用程序不再提供新输入（LZMA_FINISH），且我们不在流的中间，
    /// 且可能的流填充是 4 字节的倍数时，返回 StreamEnd
    concatenated: bool,

    /// 在解码连接流时，只要我们正在解码第一个流，此值为 true
    /// 这是为了避免在后续流没有有效魔数时产生误导性的 FormatError
    first_stream: bool,

    /// buffer 中的写入位置和流填充中的位置
    pos: usize,

    /// 用于保存流头部、块头部和流尾部的缓冲区
    /// 块头部具有最大的最大尺寸
    buffer: [u8; LZMA_BLOCK_HEADER_SIZE_MAX as usize],
}

impl Default for LzmaStreamDecoder {
    fn default() -> Self {
        LzmaStreamDecoder {
            sequence: Sequence::default(), // 假设 Sequence 实现了 Default
            block_decoder: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            block_options: LzmaBlock::default(), // 假设 LzmaBlock 实现了 Default
            stream_flags: LzmaStreamFlags::default(), // 假设 LzmaStreamFlags 实现了 Default
            index_hash: None,              // Option 类型默认是 None
            memlimit: 0,
            memusage: 0,
            tell_no_check: false,
            tell_unsupported_check: false,
            tell_any_check: false,
            ignore_check: false,
            concatenated: false,
            first_stream: false,
            pos: 0,
            buffer: [0; LZMA_BLOCK_HEADER_SIZE_MAX as usize], // 初始化为全零数组
        }
    }
}

/// 解码序列的枚举，表示解码过程中的不同阶段
#[derive(Debug, Default, Copy, Clone)]
pub enum Sequence {
    /// 解码流头部
    #[default]
    SeqStreamHeader,
    /// 解码块头部
    SeqBlockHeader,
    /// 初始化块
    SeqBlockInit,
    /// 运行块解码
    SeqBlockRun,
    /// 处理索引
    SeqIndex,
    /// 解码流尾部
    SeqStreamFooter,
    /// 处理流填充
    SeqStreamPadding,
}