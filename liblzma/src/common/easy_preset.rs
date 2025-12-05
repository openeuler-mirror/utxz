/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{
        LzmaFilter, LzmaOptionsLzma, LzmaOptionsType, LZMA_FILTERS_MAX, LZMA_FILTER_LZMA2,
        LZMA_VLI_UNKNOWN,
    },
    lzma::lzma_lzma_preset,
};

#[derive(Default)]
pub struct LzmaOptionsEasy {
    /// We need to keep the filters array available in case
    /// LZMA_FULL_FLUSH is used.
    pub filters: [LzmaFilter; LZMA_FILTERS_MAX + 1],

    /// Options for LZMA2
    pub opt_lzma: LzmaOptionsLzma,
    // More filters can be added later.
    // Currently, we're only implementing the basic structure.
}

pub fn lzma_easy_preset(opt_easy: &mut LzmaOptionsEasy, preset: u32) -> bool {
    if lzma_lzma_preset(&mut opt_easy.opt_lzma, preset) {
        return true;
    }

    // 设置过滤器为 LZMA2
    opt_easy.filters[0].id = LZMA_FILTER_LZMA2;
    opt_easy.filters[0].options = Some(LzmaOptionsType::LzmaOptionsLzma(opt_easy.opt_lzma.clone()));

    // 设置第二个过滤器为未知值
    opt_easy.filters[1].id = LZMA_VLI_UNKNOWN;

    false
}
