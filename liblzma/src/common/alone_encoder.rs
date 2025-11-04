/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::LzmaNextCoder;

const ALONE_HEADER_SIZE: usize = 1 + 4 + 8;

#[derive(Debug)]
pub struct LzmaAloneEncoder {
    next: Box<LzmaNextCoder>,
    sequence: Sequence,
    header_pos: usize,
    header: [u8; ALONE_HEADER_SIZE],
}
impl Default for LzmaAloneEncoder {
    fn default() -> Self {
        LzmaAloneEncoder {
            next: Box::new(LzmaNextCoder::default()), // Assuming LzmaNextCoder implements Default
            sequence: Sequence::default(),            // Assuming Sequence implements Default
            header_pos: 0,                            // Default value for usize
            header: [0u8; ALONE_HEADER_SIZE],         // Default array of zeros
        }
    }
}
#[derive(Debug, Default)]
enum Sequence {
    #[default]
    Header,
    Code,
}
