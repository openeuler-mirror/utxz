/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaCheck, LzmaRet, LzmaStreamFlags, LZMA_VLI_UNKNOWN},
    check::lzma_crc32,
};
use common::read32le;
use lazy_static::lazy_static;
use std::sync::Mutex;

use super::LZMA_STREAM_FLAGS_SIZE;

lazy_static! {
    pub static ref LZMA_HEADER_MAGIC: Mutex<[u8; 6]> =
        Mutex::new([0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]);
    pub static ref LZMA_FOOTER_MAGIC: Mutex<[u8; 2]> = Mutex::new([0x59, 0x5A]);
}

/// 解码流标志
fn stream_flags_decode(options: &mut LzmaStreamFlags, input: &[u8]) -> bool {
    // 保留位必须未设置
    if input[0] != 0x00 || (input[1] & 0xF0) != 0 {
        return true;
    }

    options.version = 0;
    options.check = LzmaCheck::try_from((input[1] & 0x0F) as u32).unwrap();

    false
}

/// 解码流头
pub fn lzma_stream_header_decode(options: &mut LzmaStreamFlags, input: &[u8]) -> LzmaRet {
    let lzma_head_magic = *LZMA_HEADER_MAGIC.lock().unwrap();
    // 魔数
    if input.starts_with(&lzma_head_magic) == false {
        return LzmaRet::FormatError;
    }

    // 验证 CRC32 以区分损坏和不支持的文件
    let crc = lzma_crc32(
        &input[lzma_head_magic.len()..lzma_head_magic.len() + LZMA_STREAM_FLAGS_SIZE],
        LZMA_STREAM_FLAGS_SIZE as usize,
        0,
    );
    if crc != read32le(&input[lzma_head_magic.len() + LZMA_STREAM_FLAGS_SIZE..]) {
        println!("crc: {:?}", crc);
        println!(
            "input: {:?}",
            &input[lzma_head_magic.len() + LZMA_STREAM_FLAGS_SIZE..]
        );
        return LzmaRet::DataError;
    }

    // 流标志
    if stream_flags_decode(options, &input[lzma_head_magic.len()..]) {
        return LzmaRet::OptionsError;
    }

    // 设置 Backward Size 以指示未知值
    options.backward_size = LZMA_VLI_UNKNOWN;

    LzmaRet::Ok
}

/// 解码流尾
pub fn lzma_stream_footer_decode(options: &mut LzmaStreamFlags, input: &[u8]) -> LzmaRet {
    let lzma_footer_magic = *LZMA_FOOTER_MAGIC.lock().unwrap();

    // LZMA流尾格式：
    // 字节 0-3: CRC32 (4字节)
    // 字节 4-7: Backward Size (4字节)
    // 字节 8-9: Stream Flags (2字节)
    // 字节 10-11: Footer Magic (2字节)

    // 魔数位置：从 offset = 4*2 + 2 = 10 开始

    let uint_size = std::mem::size_of::<u32>();
    let offset: usize = uint_size * 2 + LZMA_STREAM_FLAGS_SIZE; // 10
    let size_footer = lzma_footer_magic.len(); // 2

    // 检查输入缓冲区是否足够大
    if input.len() < offset + size_footer {
        return LzmaRet::DataError;
    }

    // 检查魔数，从 offset 位置开始
    if input[offset..offset + size_footer] != lzma_footer_magic {
        return LzmaRet::FormatError;
    }

    // CRC32: 计算字节4-5的CRC32，与字节0-3比较
    let crc = lzma_crc32(
        &input[uint_size..],
        uint_size + LZMA_STREAM_FLAGS_SIZE as usize,
        0,
    );
    if crc != read32le(&input[0..]) {
        return LzmaRet::DataError;
    }

    // 流标志: 解码字节8-9
    if stream_flags_decode(options, &input[8..8 + LZMA_STREAM_FLAGS_SIZE]) {
        return LzmaRet::OptionsError;
    }

    // Backward Size: 从字节4-7读取
    let backward_size = read32le(&input[uint_size..]);
    options.backward_size = (backward_size as u64 + 1) * 4;

    LzmaRet::Ok
}
