/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::{LzmaBool, LzmaCheck, LzmaFilter, LzmaReservedEnum, LzmaVli, LZMA_CHECK_SIZE_MAX};

#[derive(Debug, Clone)]
pub struct LzmaBlock {
    pub version: u32,
    pub header_size: u32,
    pub check: LzmaCheck,
    pub compressed_size: LzmaVli,
    pub uncompressed_size: LzmaVli,
    pub filters: Vec<LzmaFilter>,
    pub raw_check: [u8; LZMA_CHECK_SIZE_MAX],
    // 暂时屏蔽
    // pub reserved_ptr1: Option<T>,
    // pub reserved_ptr2: Option<T>,
    // pub reserved_ptr3: Option<T>,
    pub reserved_int1: u32,
    pub reserved_int2: u32,
    pub reserved_int3: LzmaVli,
    pub reserved_int4: LzmaVli,
    pub reserved_int5: LzmaVli,
    pub reserved_int6: LzmaVli,
    pub reserved_int7: LzmaVli,
    pub reserved_int8: LzmaVli,
    pub reserved_enum1: LzmaReservedEnum,
    pub reserved_enum2: LzmaReservedEnum,
    pub reserved_enum3: LzmaReservedEnum,
    pub reserved_enum4: LzmaReservedEnum,
    pub ignore_check: bool,
    pub reserved_bool2: LzmaBool,
    pub reserved_bool3: LzmaBool,
    pub reserved_bool4: LzmaBool,
    pub reserved_bool5: LzmaBool,
    pub reserved_bool6: LzmaBool,
    pub reserved_bool7: LzmaBool,
    pub reserved_bool8: LzmaBool,
}
#[allow(dead_code)]
impl Default for LzmaBlock {
    fn default() -> Self {
        Self {
            version: 0,
            header_size: 0,
            check: LzmaCheck::None,
            compressed_size: 0,
            uncompressed_size: 0,
            filters: Vec::new(),                   // 初始化为空向量
            raw_check: [0u8; LZMA_CHECK_SIZE_MAX], // 初始化为全零数组
            // reserved_ptr1: None,
            // reserved_ptr2: None,
            // reserved_ptr3: None,
            reserved_int1: 0,
            reserved_int2: 0,
            reserved_int3: 0,
            reserved_int4: 0,
            reserved_int5: 0,
            reserved_int6: 0,
            reserved_int7: 0,
            reserved_int8: 0,
            reserved_enum1: 0,
            reserved_enum2: 0,
            reserved_enum3: 0,
            reserved_enum4: 0,
            ignore_check: false,
            reserved_bool2: 0,
            reserved_bool3: 0,
            reserved_bool4: 0,
            reserved_bool5: 0,
            reserved_bool6: 0,
            reserved_bool7: 0,
            reserved_bool8: 0,
        }
    }
}

pub const LZMA_BLOCK_HEADER_SIZE_MIN: u32 = 8;
pub const LZMA_BLOCK_HEADER_SIZE_MAX: u32 = 1024;

#[macro_export]
macro_rules! lzma_block_header_size_decode {
    ($b:expr) => {
        (($b as u32) + 1) * 4
    };
}
