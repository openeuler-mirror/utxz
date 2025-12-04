/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaCheck, LzmaRet, LzmaStream};

use super::{lzma_easy_preset, lzma_stream_encoder, LzmaOptionsEasy};

pub fn lzma_easy_encoder(strm: &mut LzmaStream, preset: u32, check: LzmaCheck) -> LzmaRet {
    let mut opt_easy = LzmaOptionsEasy::default();

    if lzma_easy_preset(&mut opt_easy, preset) {
        return LzmaRet::OptionsError;
    }

    lzma_stream_encoder(strm, &opt_easy.filters, check)
}
