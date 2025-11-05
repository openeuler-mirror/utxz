/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::{api::LzmaOptionsLzma, lzma::LzmaLzma1Encoder};

// use super::{get_dist_slot, lzma_lzma_encoder_memusage};

/// 最大的每个块的实际数据字节数（不包括头部）
pub const LZMA2_CHUNK_MAX: u32 = 1 << 16;

/// LZMA 块的最大未压缩大小（不包括头部）
pub const LZMA2_UNCOMPRESSED_MAX: u32 = 1 << 21;

/// LZMA2 头部的最大大小
pub const LZMA2_HEADER_MAX: usize = 6;

/// 未压缩块的头部大小
pub const LZMA2_HEADER_UNCOMPRESSED: usize = 3;

/// LZMA2 编码器结构体
#[derive(Debug)]
pub struct LzmaLzma2Encoder {
    /// 编码器的状态序列
    sequence: Sequence,

    /// LZMA 编码器
    lzma: Box<LzmaLzma1Encoder>,

    /// 当前使用的 LZMA 选项
    opt_cur: LzmaOptionsLzma,

    /// 是否需要属性
    need_properties: bool,

    /// 是否需要状态重置
    need_state_reset: bool,

    /// 是否需要字典重置
    need_dictionary_reset: bool,

    /// 未压缩块的大小
    uncompressed_size: usize,

    /// 压缩块的大小（不包括头部）
    compressed_size: usize,

    /// buf[] 中的读取位置
    buf_pos: usize,

    /// 用于存储块头和 LZMA 压缩数据的缓冲区
    buf: [u8; LZMA2_HEADER_MAX + LZMA2_CHUNK_MAX as usize],
}

impl Default for LzmaLzma2Encoder {
    fn default() -> Self {
        LzmaLzma2Encoder {
            sequence: Sequence::default(),           // 假设 Sequence 实现了 Default
            lzma: Box::new(LzmaLzma1Encoder::new()), // 假设 LzmaLzDecoder 实现了 Default
            opt_cur: LzmaOptionsLzma::default(),     // 假设 LzmaOptionsLzma 实现了 Default
            need_properties: false,
            need_state_reset: false,
            need_dictionary_reset: false,
            uncompressed_size: 0,
            compressed_size: 0,
            buf_pos: 0,
            buf: [0; LZMA2_HEADER_MAX + LZMA2_CHUNK_MAX as usize], // 假设 LZMA2_HEADER_MAX 和 LZMA2_CHUNK_MAX 是常量
        }
    }
}

/// 编码器的状态序列
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum Sequence {
    #[default]
    Init,
    LzmaEncode,
    LzmaCopy,
    UncompressedHeader,
    UncompressedCopy,
}
