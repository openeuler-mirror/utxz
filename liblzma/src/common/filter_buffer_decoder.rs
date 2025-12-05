/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaAction, LzmaFilter, LzmaRet};

use super::{lzma_next_end, lzma_raw_decoder_init, LzmaNextCoder};

/// Raw buffer decoder function that processes input data through the given filters
pub fn lzma_raw_buffer_decode(
    filters: &[LzmaFilter],

    input: &mut Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // Validate input parameters
    if *in_pos > input.len() || *out_pos > out.len() {
        return LzmaRet::ProgError;
    }

    // Initialize the decoder
    let mut next = LzmaNextCoder::default();
    let ret = lzma_raw_decoder_init(&mut next, filters);
    if ret != LzmaRet::Ok {
        return ret;
    }

    // Store initial positions for potential rollback
    let in_start = *in_pos;
    let out_start = *out_pos;

    // Perform the actual decoding
    let mut ret: LzmaRet = LzmaRet::Ok;
    if let Some(code) = next.code {
        ret = code(
            &mut next.coder.as_mut().unwrap(),
            input,
            in_pos,
            in_size,
            out,
            out_pos,
            out_size,
            LzmaAction::Finish,
        );
    }

    if ret == LzmaRet::StreamEnd {
        ret = LzmaRet::Ok;
    } else if ret == LzmaRet::Ok {
        // Either input was truncated or output buffer was too small
        assert!(*in_pos == input.len() || *out_pos == out.len());

        if *in_pos != input.len() {
            // Input wasn't consumed completely, so output buffer must be too small
            ret = LzmaRet::BufError;
        } else if *out_pos != out.len() {
            // Output didn't become full, so input must be truncated
            ret = LzmaRet::DataError;
        } else {
            // All input consumed and output buffer is full
            // Try to decode one more byte to determine the actual error
            let mut tmp = [0u8; 1];
            let mut tmp_pos = 0;

            if let Some(code) = next.code {
                ret = code(
                    &mut next.coder.as_mut().unwrap(),
                    input,
                    in_pos,
                    in_size,
                    &mut tmp.to_vec(),
                    &mut tmp_pos,
                    1,
                    LzmaAction::Finish,
                );
            }

            ret = if tmp_pos == 1 {
                LzmaRet::BufError
            } else {
                LzmaRet::DataError
            };
        }

        // Restore positions on error
        *in_pos = in_start;
        *out_pos = out_start;
    }

    // Clean up
    lzma_next_end(&mut next);

    ret
}
