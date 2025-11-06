/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::{
    api::{
         LzmaOptionsLzma,
    },

};

use super::{
    LzmaNextCoder,
};

/// .lz 格式版本 0 缺少尾部中的 64 位成员大小字段
const LZIP_V0_FOOTER_SIZE: usize = 12;
const LZIP_V1_FOOTER_SIZE: usize = 20;
const LZIP_FOOTER_SIZE_MAX: usize = LZIP_V1_FOOTER_SIZE;

// lc/lp/pb 在 .lz 格式中是硬编码的
const LZIP_LC: u32 = 3;
const LZIP_LP: u32 = 0;
const LZIP_PB: u32 = 2;

/// 解码过程中的状态序列
#[derive(Debug, Clone, Copy, PartialEq)]
enum DecodingSequence {
    SeqIdString,     // 解码 ID 字符串
    SeqVersion,      // 解码版本
    SeqDictSize,     // 解码字典大小
    SeqCoderInit,    // 解码器初始化
    SeqLzmaStream,   // 解码 LZMA 流
    SeqMemberFooter, // 解码成员尾部
}

/// LZMA 解码器结构体，用于处理 .lz 格式
#[derive(Debug)]
pub struct LzmaLzipCoder {
    /// 当前解码状态
    sequence: DecodingSequence,

    /// .lz 成员格式版本
    version: u32,

    /// 解压后的数据 CRC32 校验和
    crc32: u32,

    /// 解压后的数据大小
    uncompressed_size: u64,

    /// 成员的压缩大小
    member_size: u64,

    /// 内存使用限制
    memlimit: u64,

    /// 实际需要的内存量
    memusage: u64,

    /// 如果为 true，则在解码头部字段后返回 LZMA_GET_CHECK
    tell_any_check: bool,

    /// 如果为 true，则跳过 CRC32 校验
    ignore_check: bool,

    /// 如果为 true，则解码连接的 .lz 成员，并在解码第一个成员后遇到非 .lz 数据时停止
    concatenated: bool,

    /// 在解码连接的 .lz 成员时，表示当前正在解码第一个 .lz 成员
    first_member: bool,

    /// 当前头部和尾部字段的读取位置
    pos: usize,

    /// 用于存储 .lz 文件尾部字段的缓冲区
    buffer: [u8; LZIP_FOOTER_SIZE_MAX],

    /// 从 .lz 头部解码的选项，用于初始化 LZMA1 解码器
    options: LzmaOptionsLzma,

    /// LZMA1 解码器实例
    lzma_decoder: Box<LzmaNextCoder>,
}

impl Default for LzmaLzipCoder {
    fn default() -> Self {
        Self {
            sequence: DecodingSequence::SeqIdString,
            version: 0,
            crc32: 0,
            uncompressed_size: 0,
            member_size: 0,
            memlimit: 0,
            memusage: 0,
            tell_any_check: false,
            ignore_check: false,
            concatenated: false,
            first_member: true,
            pos: 0,
            buffer: [0; LZIP_FOOTER_SIZE_MAX],
            options: LzmaOptionsLzma::default(),
            lzma_decoder: Box::new(LzmaNextCoder::default()),
        }
    }
}
