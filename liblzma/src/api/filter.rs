/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::any::Any;

use crate::api::LzmaVli;

use super::{LzmaOptionsBcj, LzmaOptionsDelta, LzmaOptionsLzma};

pub const LZMA_FILTERS_MAX: usize = 4;

#[derive(Debug, Clone)]
pub enum LzmaOptionsType {
    LzmaOptionsLzma(LzmaOptionsLzma),
    Delta(LzmaOptionsDelta),
    Bcj(LzmaOptionsBcj),
    Lod(LzmaOptionsDelta),

    None,
}

impl LzmaOptionsType {
    pub fn as_lzma_options_lzma(&self) -> Option<&LzmaOptionsLzma> {
        match self {
            LzmaOptionsType::LzmaOptionsLzma(ref opts) => Some(opts),
            _ => None,
        }
    }

    pub fn as_delta(&self) -> Option<&LzmaOptionsDelta> {
        match self {
            LzmaOptionsType::Delta(ref opts) => Some(opts),
            _ => None,
        }
    }

    pub fn as_bcj(&self) -> Option<&LzmaOptionsBcj> {
        match self {
            LzmaOptionsType::Bcj(ref opts) => Some(opts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LzmaFilter {
    pub id: LzmaVli,
    pub options: Option<LzmaOptionsType>,
}
impl Default for LzmaFilter {
    fn default() -> Self {
        LzmaFilter {
            id: LzmaVli::default(), // 假设 LzmaVli 实现了 Default
            options: None,          // Option 类型默认是 None
        }
    }
}

pub const LZMA_STR_ALL_FILTERS: u32 = 0x01;
pub const LZMA_STR_NO_VALIDATION: u32 = 0x02;
pub const LZMA_STR_ENCODER: u32 = 0x10;
pub const LZMA_STR_DECODER: u32 = 0x20;
pub const LZMA_STR_GETOPT_LONG: u32 = 0x40;
pub const LZMA_STR_NO_SPACES: u32 = 0x80;
