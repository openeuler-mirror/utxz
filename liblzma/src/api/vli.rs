/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

pub type LzmaVli = u64;
pub const LZMA_VLI_MAX: u64 = u64::MAX / 2;
pub const LZMA_VLI_UNKNOWN: u64 = u64::MAX;
pub const LZMA_VLI_BYTES_MAX: usize = 9;

#[macro_export]
macro_rules! LZMA_VLI_C {
    ($n:expr) => {
        $n as u64
    };
}

pub fn lzma_vli_is_valid(vli: u64) -> bool {
    vli <= LZMA_VLI_MAX || vli == LZMA_VLI_UNKNOWN
}
