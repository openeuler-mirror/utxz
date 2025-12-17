/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::write32le;

use crate::{
    api::{LzmaRet, LzmaStreamFlags, LZMA_CHECK_ID_MAX, LZMA_STREAM_HEADER_SIZE},
    check::lzma_crc32,
    common::{
        is_backward_size_valid, LZMA_FOOTER_MAGIC, LZMA_HEADER_MAGIC, LZMA_STREAM_FLAGS_SIZE,
    },
};

/// 编码流标志
fn stream_flags_encode(options: &LzmaStreamFlags, out: &mut [u8]) -> bool {
    if options.check.clone() as u32 > LZMA_CHECK_ID_MAX {
        return true;
    }

    out[0] = 0x00;
    out[1] = options.check.clone() as u8;

    false
}

/// 编码流头
pub fn lzma_stream_header_encode(options: &LzmaStreamFlags, out: &mut [u8]) -> LzmaRet {
    let lzma_header_magic = *LZMA_HEADER_MAGIC.lock().unwrap();
    assert_eq!(
        lzma_header_magic.len() + LZMA_STREAM_FLAGS_SIZE + 4,
        LZMA_STREAM_HEADER_SIZE
    );

    if options.version != 0 {
        return LzmaRet::OptionsError;
    }

    // 魔数
    out[0..lzma_header_magic.len()].copy_from_slice(&lzma_header_magic);

    // 流标志
    if stream_flags_encode(options, &mut out[lzma_header_magic.len()..]) {
        return LzmaRet::ProgError;
    }

    // 流头的 CRC32
    let crc = lzma_crc32(
        &out[lzma_header_magic.len()..lzma_header_magic.len() + LZMA_STREAM_FLAGS_SIZE],
        LZMA_STREAM_FLAGS_SIZE as usize,
        0,
    );
    write32le(
        &mut out[lzma_header_magic.len() + LZMA_STREAM_FLAGS_SIZE..],
        crc,
    );
    LzmaRet::Ok
}

/// 编码流尾
pub fn lzma_stream_footer_encode(options: &mut LzmaStreamFlags, out: &mut [u8]) -> LzmaRet {
    let lzma_footer_magic = *LZMA_FOOTER_MAGIC.lock().unwrap();
    assert_eq!(
        2 * 4 + LZMA_STREAM_FLAGS_SIZE + lzma_footer_magic.len(),
        LZMA_STREAM_HEADER_SIZE
    );

    if options.version != 0 {
        return LzmaRet::OptionsError;
    }

    // Backward Size
    if !is_backward_size_valid(options) {
        return LzmaRet::ProgError;
    }

    write32le(&mut out[4..], options.backward_size as u32 / 4 - 1);

    // 流标志
    if stream_flags_encode(options, &mut out[2 * 4..]) {
        return LzmaRet::ProgError;
    }

    // CRC32
    let crc = lzma_crc32(&out[4..], 4 + LZMA_STREAM_FLAGS_SIZE as usize, 0);
    write32le(out, crc);

    // 魔数
    out[2 * 4 + LZMA_STREAM_FLAGS_SIZE..2 * 4 + LZMA_STREAM_FLAGS_SIZE + 2]
        .copy_from_slice(&lzma_footer_magic);

    LzmaRet::Ok
}
