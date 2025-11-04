/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{api::LzmaOptionsLzma, lz::LzmaLzDecoder};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sequence {
    #[default]
    Control,
    Uncompressed1,
    Uncompressed2,
    Compressed0,
    Compressed1,
    Properties,
    Lzma,
    Copy,
}
#[derive(Debug)]
pub struct LzmaLzma2Decoder {
    sequence: Sequence,
    next_sequence: Sequence,
    lzma: Box<LzmaLzDecoder>,
    uncompressed_size: usize,
    compressed_size: usize,
    need_properties: bool,
    need_dictionary_reset: bool,
    options: LzmaOptionsLzma,
}

impl Default for LzmaLzma2Decoder {
    fn default() -> Self {
        LzmaLzma2Decoder {
            sequence: Sequence::default(),      // 假设 Sequence 实现了 Default
            next_sequence: Sequence::default(), // 假设 Sequence 实现了 Default
            lzma: Box::new(LzmaLzDecoder::default()), // 假设 LzmaLzDecoder 实现了 Default
            uncompressed_size: 0,
            compressed_size: 0,
            need_properties: false,
            need_dictionary_reset: false,
            options: LzmaOptionsLzma::default(), // 假设 LzmaOptionsLzma 实现了 Default
        }
    }
}
