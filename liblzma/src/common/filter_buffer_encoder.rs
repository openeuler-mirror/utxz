/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaAction, LzmaFilter, LzmaRet};

use super::{lzma_next_end, lzma_raw_decoder_init, LzmaNextCoder};

/// Raw buffer encoder function that processes input data through the given filters
pub fn lzma_raw_buffer_encode(
    filters: &[LzmaFilter],
    input: &mut Vec<u8>,
    in_size: usize,
    out: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // Validate parameters
    if *out_pos > out.len() {
        return LzmaRet::ProgError;
    }

    // Initialize the encoder
    let mut next = LzmaNextCoder::default();
    let ret = lzma_raw_decoder_init(&mut next, filters);
    if ret != LzmaRet::Ok {
        return ret;
    }

    // Store the output position for potential rollback
    let out_start = *out_pos;

    // Perform the actual encoding
    let mut in_pos = 0;

    let mut ret: LzmaRet = LzmaRet::Ok;
    if let Some(code) = next.code {
        ret = code(
            &mut next.coder.as_mut().unwrap(),
            input,
            &mut in_pos,
            in_size,
            out,
            out_pos,
            out_size,
            LzmaAction::Finish,
        );
    }

    // Clean up encoder resources
    lzma_next_end(&mut next);

    if ret == LzmaRet::StreamEnd {
        // Encode completed successfully
        return LzmaRet::Ok;
    } else {
        if ret == LzmaRet::Ok {
            // Restore output position on error
            assert!(*out_pos == out_size);
            ret = LzmaRet::BufError;
        }

        *out_pos = out_start;
    }

    ret
}
