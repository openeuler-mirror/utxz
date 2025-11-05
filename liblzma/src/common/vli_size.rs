/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::{LzmaVli, LZMA_VLI_BYTES_MAX, LZMA_VLI_MAX};

/// 计算可变长度整数的字节大小
pub fn lzma_vli_size(mut vli: LzmaVli) -> u32 {
    if vli > LZMA_VLI_MAX {
        return 0;
    }

    let mut i: u32 = 0;
    loop {
        vli >>= 7;
        i += 1;
        if vli == 0 {
            break;
        }
    }

    assert!(i <= LZMA_VLI_BYTES_MAX as u32);
    i
}
