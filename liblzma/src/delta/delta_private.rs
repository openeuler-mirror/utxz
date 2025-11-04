/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{api::LZMA_DELTA_DIST_MAX, common::LzmaNextCoder};

#[derive(Debug)]
pub struct LzmaDeltaCoder {
    pub next: Box<LzmaNextCoder>,
    pub distance: usize,
    pub pos: u8,
    pub history: [u8; LZMA_DELTA_DIST_MAX as usize],
}

impl Default for LzmaDeltaCoder {
    fn default() -> Self {
        LzmaDeltaCoder {
            next: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            distance: 0,
            pos: 0,
            history: [0; LZMA_DELTA_DIST_MAX as usize], // 全零数组
        }
    }
}
