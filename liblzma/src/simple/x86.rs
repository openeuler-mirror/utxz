/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       x86.rs
/// \brief      Filter for x86 binaries (BCJ filter)
///
//  Authors:    Igor Pavlov
//              Lasse Collin
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////
use std::ptr;

use crate::{
    api::LzmaRet,
    common::{CoderType, LzmaFilterInfo, LzmaNextCoder},
};

use super::{lzma_simple_coder_init, SimpleType};

#[derive(Default)]
struct LzmaSimpleX86 {
    prev_mask: u32,
    prev_pos: u32,
}

const MASK_TO_ALLOWED_STATUS: [bool; 8] = [true, true, true, false, true, false, false, false];
const MASK_TO_BIT_NUMBER: [u32; 8] = [0, 1, 2, 2, 3, 3, 3, 3];

/// Helper function to check if the byte is 0 or 0xFF
fn test_86_ms_byte(b: u8) -> bool {
    b == 0 || b == 0xFF
}

fn x86_code(
    simple_ptr: &mut SimpleType,
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let simple = match simple_ptr {
        SimpleType::X86Filter(ref mut s) => s,
        _ => return 0,
    };
    let mut prev_mask = simple.prev_mask;
    let mut prev_pos = simple.prev_pos;

    if size < 5 {
        return 0;
    }

    if now_pos - prev_pos > 5 {
        prev_pos = now_pos - 5;
    }

    let limit = size - 5;
    let mut buffer_pos = 0;

    while buffer_pos <= limit {
        let mut b = buffer[buffer_pos];
        if b != 0xE8 && b != 0xE9 {
            buffer_pos += 1;
            continue;
        }

        let offset = now_pos + buffer_pos as u32 - prev_pos;
        prev_pos = now_pos + buffer_pos as u32;

        if offset > 5 {
            prev_mask = 0;
        } else {
            for i in 0..offset {
                prev_mask &= 0x77;
                prev_mask <<= 1;
            }
        }

        b = buffer[buffer_pos + 4];

        if test_86_ms_byte(b)
            && MASK_TO_ALLOWED_STATUS[(prev_mask >> 1) as usize & 0x7]
            && (prev_mask >> 1) < 0x10
        {
            let mut src = ((b as u32) << 24)
                | ((buffer[buffer_pos + 3] as u32) << 16)
                | ((buffer[buffer_pos + 2] as u32) << 8)
                | (buffer[buffer_pos + 1] as u32);

            let mut dest;
            loop {
                dest = if is_encoder {
                    src + (now_pos + buffer_pos as u32 + 5)
                } else {
                    src - (now_pos + buffer_pos as u32 + 5)
                };

                if prev_mask == 0 {
                    break;
                }

                let i = MASK_TO_BIT_NUMBER[(prev_mask >> 1) as usize];
                b = (dest >> (24 - i * 8)) as u8;

                if !test_86_ms_byte(b) {
                    break;
                }

                src = dest ^ ((1u32 << (32 - i * 8)) - 1);
            }

            buffer[buffer_pos + 4] = !(dest >> 24 & 1) as u8;
            buffer[buffer_pos + 3] = (dest >> 16) as u8;
            buffer[buffer_pos + 2] = (dest >> 8) as u8;
            buffer[buffer_pos + 1] = dest as u8;
            buffer_pos += 5;
            prev_mask = 0;
        } else {
            buffer_pos += 1;
            prev_mask |= 1;
            if test_86_ms_byte(b) {
                prev_mask |= 0x10;
            }
        }
    }

    simple.prev_mask = prev_mask;
    simple.prev_pos = prev_pos;

    buffer_pos
}

fn x86_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    let ret = lzma_simple_coder_init(
        next,
        filters,
        x86_code,
        std::mem::size_of::<LzmaSimpleX86>(),
        5,
        1,
        is_encoder,
    );

    if ret == LzmaRet::Ok {
        let coder = match next.coder.as_mut().unwrap() {
            CoderType::SimpleCoder(ref mut s) => s,
            _ => return LzmaRet::ProgError,
        };
        let simple = match coder.simple {
            SimpleType::X86Filter(ref mut s) => s,
            _ => return LzmaRet::ProgError,
        };
        simple.prev_mask = 0;
        simple.prev_pos = u32::MAX - 5;
    }

    ret
}

pub fn lzma_simple_x86_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    x86_coder_init(next, filters, true)
}

pub fn lzma_simple_x86_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    x86_coder_init(next, filters, false)
}
