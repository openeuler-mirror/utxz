/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::{
    LzmaMatchFinder, LzmaMode, LzmaOptionsLzma, LZMA_LC_DEFAULT, LZMA_LP_DEFAULT, LZMA_PB_DEFAULT,
    LZMA_PRESET_EXTREME, LZMA_PRESET_LEVEL_MASK,
};

pub fn lzma_lzma_preset(options: &mut LzmaOptionsLzma, preset: u32) -> bool {
    let level = preset & LZMA_PRESET_LEVEL_MASK;
    let flags = preset & !LZMA_PRESET_LEVEL_MASK;
    let supported_flags = LZMA_PRESET_EXTREME;

    if level > 9 || (flags & !supported_flags) != 0 {
        return true;
    }

    options.preset_dict = None;
    options.preset_dict_size = 0;

    options.lc = LZMA_LC_DEFAULT;
    options.lp = LZMA_LP_DEFAULT;
    options.pb = LZMA_PB_DEFAULT;

    let dict_pow2 = [18, 20, 21, 22, 22, 23, 23, 24, 25, 26];
    options.dict_size = 1u32 << dict_pow2[level as usize];

    if level <= 3 {
        options.mode = LzmaMode::Fast;
        options.mf = if level == 0 {
            LzmaMatchFinder::LzmaMfHc3
        } else {
            LzmaMatchFinder::LzmaMfHc4
        };
        options.nice_len = if level <= 1 { 128 } else { 273 };
        let depths = [4, 8, 24, 48];
        options.depth = depths[level as usize];
    } else {
        options.mode = LzmaMode::Normal;
        options.mf = LzmaMatchFinder::LzmaMfBt4;
        options.nice_len = match level {
            4 => 16,
            5 => 32,
            _ => 64,
        };
        options.depth = 0;
    }

    if flags & LZMA_PRESET_EXTREME != 0 {
        options.mode = LzmaMode::Normal;
        options.mf = LzmaMatchFinder::LzmaMfBt4;
        if level == 3 || level == 5 {
            options.nice_len = 192;
            options.depth = 0;
        } else {
            options.nice_len = 273;
            options.depth = 512;
        }
    }

    false
}
