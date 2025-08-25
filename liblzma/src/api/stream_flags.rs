/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::LZMA_VLI_C;

use super::{LzmaBool, LzmaCheck, LzmaReservedEnum, LzmaVli};
#[derive(Debug, Default, Clone)]
pub struct LzmaStreamFlags {
    pub version: u32,
    pub backward_size: LzmaVli,
    pub check: LzmaCheck,
    pub reserved_enum1: LzmaReservedEnum,
    pub reserved_enum2: LzmaReservedEnum,
    pub reserved_enum3: LzmaReservedEnum,
    pub reserved_enum4: LzmaReservedEnum,
    pub reserved_bool1: LzmaBool,
    pub reserved_bool2: LzmaBool,
    pub reserved_bool3: LzmaBool,
    pub reserved_bool4: LzmaBool,
    pub reserved_bool5: LzmaBool,
    pub reserved_bool6: LzmaBool,
    pub reserved_bool7: LzmaBool,
    pub reserved_bool8: LzmaBool,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    // pub(crate) tmp: LzmaCheck,
}

pub const LZMA_BACKWARD_SIZE_MIN: u32 = 4;
pub const LZMA_BACKWARD_SIZE_MAX: LzmaVli = LZMA_VLI_C!(1) << 34;
/// LZMA 流头的大小
pub const LZMA_STREAM_HEADER_SIZE: usize = 12;
