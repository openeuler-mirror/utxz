/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaAllocator, LzmaCheck, LzmaRet};

use super::{lzma_easy_preset, lzma_stream_buffer_encode, LzmaOptionsEasy};

pub fn lzma_easy_buffer_encode(
    preset: u32,
    check: u32,
    allocator: &LzmaAllocator,
    in_data: &mut Vec<u8>,
    in_size: usize,
    out_data: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 创建一个 `lzma_options_easy` 结构体
    let mut opt_easy = LzmaOptionsEasy::default();

    // 假设 `lzma_easy_preset` 是一个函数，它将 preset 应用到 `opt_easy`
    if lzma_easy_preset(&mut opt_easy, preset) {
        return LzmaRet::OptionsError;
    }

    // 调用 `lzma_stream_buffer_encode` 处理编码
    lzma_stream_buffer_encode(
        &opt_easy.filters,
        LzmaCheck::try_from(check).unwrap(),
        allocator,
        in_data,
        in_size,
        out_data,
        out_pos,
        out_size,
    )
}
