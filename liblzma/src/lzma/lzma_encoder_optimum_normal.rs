/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::{my_max, my_min};

use crate::{
    common::lzma_memcmplen,
    get_dist_state,
    lz::{lzma_mf_find, mf_avail, mf_skip, LzmaMf},
    lzma::OPTS,
    not_equal_16,
    rangecoder::{
        rc_bit_0_price, rc_bit_1_price, rc_bit_price, rc_bittree_price, rc_bittree_reverse_price,
        rc_direct_price, RC_INFINITY_PRICE,
    },
};

use super::{
    get_dist_slot, get_dist_slot_2, is_literal_state, update_literal, update_long_rep,
    update_match, update_short_rep, LzmaLengthEncoder, LzmaLzma1Encoder, LzmaOptimal, ALIGN_BITS,
    ALIGN_MASK, ALIGN_SIZE, DIST_MODEL_END, DIST_MODEL_START, DIST_SLOT_BITS, DIST_STATES,
    FULL_DISTANCES, MATCH_LEN_MAX, MATCH_LEN_MIN, REPS,
};
use crate::lzma::LIT_STATES;

pub fn lzma_lzma_optimum_normal(
    coder: &mut LzmaLzma1Encoder,
    mf: &mut LzmaMf,
    back_res: &mut u32,
    len_res: &mut u32,
    position: u32,
) {
    // If we have symbols pending, return the next pending symbol.
    if coder.opts_end_index != coder.opts_current_index {
        assert!(mf.read_ahead > 0);
        *len_res =
            coder.opts[coder.opts_current_index as usize].pos_prev - coder.opts_current_index;
        *back_res = coder.opts[coder.opts_current_index as usize].back_prev;
        coder.opts_current_index = coder.opts[coder.opts_current_index as usize].pos_prev;
        return;
    }

    // Update the price tables. In LZMA SDK <= 4.60 (and possibly later)
    // this was done in both initialization function and in the main loop.
    // In liblzma they were moved into this single place.
    if mf.read_ahead == 0 {
        if coder.match_price_count >= (1 << 7) {
            fill_dist_prices(coder);
        }

        if coder.align_price_count >= ALIGN_SIZE as u32 {
            fill_align_prices(coder);
        }
    }

    // TODO: This needs quite a bit of cleaning still. But splitting
    // the original function into two pieces makes it at least a little
    // more readable, since those two parts don't share many variables.
    let mut len_end = helper1(coder, mf, back_res, len_res, position);
    if len_end == u32::MAX {
        return;
    }

    let mut reps = [0; REPS];
    reps.copy_from_slice(&coder.reps);

    let mut cur = 1;
    while cur < len_end {
        assert!(cur < OPTS as u32);

        coder.longest_match_length = lzma_mf_find(mf, &mut coder.matches_count, &mut coder.matches);

        if coder.longest_match_length >= mf.nice_len {
            break;
        }

        len_end = helper2(
            coder,
            &mut reps,
            mf,
            len_end,
            position + cur,
            cur,
            std::cmp::min(mf_avail(mf) + 1, OPTS as u32 - 1 - cur),
        );
        cur += 1;
    }

    backward(coder, len_res, back_res, cur);
    return;
}

/// 辅助函数 2
fn helper2(
    coder: &mut LzmaLzma1Encoder,
    reps: &mut [u32; REPS],
    mf: &mut LzmaMf,
    mut len_end: u32,
    position: u32,
    cur: u32,
    buf_avail_full: u32,
) -> u32 {
    0
}

fn backward(coder: &mut LzmaLzma1Encoder, len_res: &mut u32, back_res: &mut u32, mut cur: u32) {

}

fn fill_dist_prices(coder: &mut LzmaLzma1Encoder){}

/// 填充对齐价格
fn fill_align_prices(coder: &mut LzmaLzma1Encoder) {
    for i in 0..ALIGN_SIZE {
        coder.align_prices[i as usize] =
            rc_bittree_reverse_price(&coder.dist_align, ALIGN_BITS as u32, i as u32, 0);
    }

    coder.align_price_count = 0;
}

/// 辅助函数 1
fn helper1(
    coder: &mut LzmaLzma1Encoder,
    mf: &mut LzmaMf,
    back_res: &mut u32,
    len_res: &mut u32,
    position: u32,
) -> u32 {
    0
}