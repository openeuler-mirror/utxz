/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaOptionsLzma, LzmaVli};
use crate::common::LzmaNextCoder;

#[repr(i32)]
#[derive(Clone, PartialEq, Debug, Default)]
pub enum Sequence {
    #[default]
    Properties,
    DictionarySize,
    UncompressedSize,
    CoderInit,
    Code,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct LzmaAloneDecoder {
    pub next: Box<LzmaNextCoder>,
    pub sequence: Sequence,
    pub picky: bool,
    pub pos: usize,
    pub uncompressed_size: LzmaVli,
    pub memlimit: u64,
    pub memusage: u64,
    pub options: LzmaOptionsLzma,
}
