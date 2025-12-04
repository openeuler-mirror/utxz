/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::{lzma_easy_preset, lzma_raw_decoder_memusage, LzmaOptionsEasy};

pub fn lzma_easy_decoder_memusage(preset: u32) -> u64 {
    let mut opt_easy = LzmaOptionsEasy::default();

    if lzma_easy_preset(&mut opt_easy, preset) {
        return u32::MAX as u64;
    }

    lzma_raw_decoder_memusage(&opt_easy.filters)
}
