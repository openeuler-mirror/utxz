/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::{LzmaCheck, LzmaFilter, LzmaReservedEnum};

pub const LZMA_PRESET_DEFAULT: u32 = 6;
pub const LZMA_PRESET_LEVEL_MASK: u32 = 0x1F;
pub const LZMA_PRESET_EXTREME: u32 = 1 << 31;

pub struct LzmaMt<'a> {
    pub flags: u32,
    pub threads: u32,
    pub block_size: u64,
    pub timeout: u32,
    pub preset: u32,
    pub filters: &'a mut LzmaFilter,
    pub check: LzmaCheck,
    pub reserved_enum1: LzmaReservedEnum,
    pub reserved_enum2: LzmaReservedEnum,
    pub reserved_enum3: LzmaReservedEnum,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    pub reserved_int3: u32,
    pub reserved_int4: u32,
    pub memlimit_threading: u64,
    pub memlimit_stop: u64,
    pub reserved_int7: u64,
    pub reserved_int8: u64,
    // 暂时屏蔽
    // pub reserved_ptr1: Box<T>,
    // pub reserved_ptr2: Box<T>,
    // pub reserved_ptr3: Box<T>,
    // pub reserved_ptr4: Box<T>,
}

pub const LZMA_TELL_NO_CHECK: u32 = 0x01;
pub const LZMA_TELL_UNSUPPORTED_CHECK: u32 = 0x02;
pub const LZMA_TELL_ANY_CHECK: u32 = 0x04;
pub const LZMA_IGNORE_CHECK: u32 = 0x10;
pub const LZMA_CONCATENATED: u32 = 0x08;
pub const LZMA_FAIL_FAST: u32 = 0x20;
