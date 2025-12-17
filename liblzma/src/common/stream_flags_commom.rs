/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{
    LzmaRet, LzmaStreamFlags, LZMA_BACKWARD_SIZE_MAX, LZMA_BACKWARD_SIZE_MIN, LZMA_CHECK_ID_MAX,
    LZMA_VLI_UNKNOWN,
};
use lazy_static::lazy_static;
use std::sync::Mutex;

// 常量定义
pub const LZMA_STREAM_FLAGS_SIZE: usize = 2;

#[inline]
pub fn is_backward_size_valid(options: &LzmaStreamFlags) -> bool {
    options.backward_size >= LZMA_BACKWARD_SIZE_MIN as u64
        && options.backward_size <= LZMA_BACKWARD_SIZE_MAX
        && (options.backward_size & 3) == 0
}

// pub static LZMA_HEADER_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
// pub static LZMA_FOOTER_MAGIC: [u8; 2] = [0x59, 0x5A];

pub fn lzma_stream_flags_compare(mut a: &LzmaStreamFlags, mut b: &LzmaStreamFlags) -> LzmaRet {
    // 只能比较版本0的结构体
    if a.version != 0 || b.version != 0 {
        return LzmaRet::OptionsError;
    }

    // 检查类型
    if a.check.clone() as u32 > LZMA_CHECK_ID_MAX || b.check.clone() as u32 > LZMA_CHECK_ID_MAX {
        return LzmaRet::ProgError;
    }

    if a.check != b.check {
        return LzmaRet::DataError;
    }

    // 只有当两个backward_size都已知时才进行比较
    if a.backward_size != LZMA_VLI_UNKNOWN && b.backward_size != LZMA_VLI_UNKNOWN {
        if !is_backward_size_valid(&mut a) || !is_backward_size_valid(&mut b) {
            return LzmaRet::ProgError;
        }

        if a.backward_size != b.backward_size {
            return LzmaRet::DataError;
        }
    }

    LzmaRet::Ok
}
