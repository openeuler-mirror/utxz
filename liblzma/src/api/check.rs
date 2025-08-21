/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use num_enum::TryFromPrimitive;

#[derive(Clone, Copy, Default, Debug, PartialEq, TryFromPrimitive, PartialOrd)]
#[repr(u32)]
pub enum LzmaCheck {
    #[default]
    None = 0,
    Crc32 = 1,
    Crc64 = 4,
    Sha256 = 10,
}

pub const LZMA_CHECK_ID_MAX: u32 = 15;
pub const LZMA_CHECK_SIZE_MAX: usize = 64;
