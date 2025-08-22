/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::LZMA_VLI_C;

use super::LzmaVli;

pub const LZMA_FILTER_DELTA: LzmaVli = LZMA_VLI_C!(0x03);

#[derive(PartialEq, Debug, Default, Clone)]
pub enum LzmaDeltaType {
    #[default]
    Byte,
}

#[derive(Debug, Default, Clone)]
pub struct LzmaOptionsDelta {
    pub type_: LzmaDeltaType,
    pub dist: u32,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    pub reserved_int3: u32,
    pub reserved_int4: u32,
    // pub reserved_ptr1: Option<T>,
    // pub reserved_ptr2: Option<T>,
}

pub const LZMA_DELTA_DIST_MIN: u32 = 1;
pub const LZMA_DELTA_DIST_MAX: u32 = 256;
