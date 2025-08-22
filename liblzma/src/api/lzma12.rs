/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{api::LzmaVli, LZMA_VLI_C};

use super::LzmaReservedEnum;

pub const LZMA_FILTER_LZMA1: LzmaVli = LZMA_VLI_C!(0x4000000000000001);
pub const LZMA_FILTER_LZMA1EXT: LzmaVli = LZMA_VLI_C!(0x4000000000000002);
pub const LZMA_FILTER_LZMA2: LzmaVli = LZMA_VLI_C!(0x21);

#[derive(PartialEq, Debug, Clone, Default)]
pub enum LzmaMatchFinder {
    #[default]
    LzmaMfHc3 = 0x03,
    LzmaMfHc4 = 0x04,
    LzmaMfBt2 = 0x12,
    LzmaMfBt3 = 0x13,
    LzmaMfBt4 = 0x14,
}

pub const LZMA_MF_HC3: u64 = 0x03;
pub const LZMA_MF_HC4: u64 = 0x04;
pub const LZMA_MF_BT2: u64 = 0x12;
pub const LZMA_MF_BT3: u64 = 0x13;
pub const LZMA_MF_BT4: u64 = 0x14;

#[derive(PartialEq, Debug, Default, Clone)]
pub enum LzmaMode {
    #[default]
    Fast = 1,
    Normal = 2,
}

pub const LZMA_MODE_FAST: u64 = 1;
pub const LZMA_MODE_NORMAL: u64 = 2;

pub const LZMA_LCLP_MIN: u32 = 0;
pub const LZMA_LCLP_MAX: u32 = 4;
pub const LZMA_LC_DEFAULT: u32 = 3;
pub const LZMA_LP_DEFAULT: u32 = 0;
pub const LZMA_PB_MIN: u32 = 0;
pub const LZMA_PB_MAX: u32 = 4;
pub const LZMA_PB_DEFAULT: u32 = 2;
#[repr(C)]
#[derive(Debug, Default)]
pub struct LzmaOptionsLzma {
    pub dict_size: u32,
    pub preset_dict: Option<Vec<u8>>,
    pub preset_dict_size: u32,
    pub lc: u32,
    pub lp: u32,
    pub pb: u32,
    pub mode: LzmaMode,
    pub nice_len: u32,
    pub mf: LzmaMatchFinder,
    pub depth: u32,
    pub ext_flags: u32,
    pub ext_size_low: u32,
    pub ext_size_high: u32,
    pub reserved_int4: u32,
    pub reserved_int5: u32,
    pub reserved_int6: u32,
    pub reserved_int7: u32,
    pub reserved_int8: u32,
    pub reserved_enum1: LzmaReservedEnum,
    pub reserved_enum2: LzmaReservedEnum,
    pub reserved_enum3: LzmaReservedEnum,
    pub reserved_enum4: LzmaReservedEnum,
    // 下面两个是保留字段，暂时不使用
    // pub reserved_ptr1: Option<T>,
    // pub reserved_ptr2: Option<T>,
}

impl Clone for LzmaOptionsLzma {
    fn clone(&self) -> Self {
        Self {
            dict_size: self.dict_size,
            preset_dict: self.preset_dict.clone(),
            preset_dict_size: self.preset_dict_size,
            lc: self.lc,
            lp: self.lp,
            pb: self.pb,
            mode: self.mode.clone(),
            nice_len: self.nice_len,
            mf: self.mf.clone(),
            depth: self.depth,
            ext_flags: self.ext_flags,
            ext_size_low: self.ext_size_low,
            ext_size_high: self.ext_size_high,
            reserved_int4: self.reserved_int4,
            reserved_int5: self.reserved_int5,
            reserved_int6: self.reserved_int6,
            reserved_int7: self.reserved_int7,
            reserved_int8: self.reserved_int8,
            reserved_enum1: self.reserved_enum1,
            reserved_enum2: self.reserved_enum2,
            reserved_enum3: self.reserved_enum3,
            reserved_enum4: self.reserved_enum4,
            // reserved_ptr1: None, // 或者根据需求实现自定义逻辑
            // reserved_ptr2: None, // 或者根据需求实现自定义逻辑
        }
    }
}
pub const LZMA_LZMA1EXT_ALLOW_EOPM: u32 = 0x01;

pub fn lzma_set_ext_size(opt_lzma2: &mut LzmaOptionsLzma, u64size: u64) {
    opt_lzma2.ext_size_low = u64size as u32;
    opt_lzma2.ext_size_high = (u64size >> 32) as u32;
}

pub const LZMA_DICT_SIZE_MIN: u32 = 4096;
pub const LZMA_DICT_SIZE_DEFAULT: u32 = 1 << 23;
