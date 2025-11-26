/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::{my_max, my_min};

use crate::{
    common::lzma_memcmplen,
    lz::{mf_avail, mf_skip, LzmaMf, MF_FIND},
    not_equal_16,
};

use super::{LzmaLzma1Encoder, MATCH_LEN_MAX, REPS};

const fn change_pair(small_dist: u32, big_dist: u32) -> bool {
    (big_dist >> 7) > small_dist
}

pub fn lzma_lzma_optimum_fast(
    coder: &mut LzmaLzma1Encoder,
    mf: &mut LzmaMf,
    back_res: &mut u32,
    len_res: &mut u32,
) {
    let nice_len = mf.nice_len;

    let mut len_main;
    let mut matches_count = 0;
    if mf.read_ahead == 0 {
        len_main = MF_FIND(&mut mf.clone(), &mut matches_count, &mut coder.matches);
    } else {
        assert!(mf.read_ahead == 1);
        len_main = coder.longest_match_length;
        matches_count = coder.matches_count;
    }

    // const uint8_t *buf = mf_ptr(mf) - 1; 有 -1操作
    let mut buf = &mf.buffer[mf.mf_ptr(1)..];
    let buf_avail = my_min(mf_avail(mf) + 1, MATCH_LEN_MAX as u32);

    if buf_avail < 2 {
        *back_res = u32::MAX;
        *len_res = 1;
        return;
    }

    let mut rep_len = 0;
    let mut rep_index = 0;

    for i in 0..REPS {
        let buf_back = &buf[..buf.len() - coder.reps[i] as usize - 1];

        if not_equal_16!(buf, buf_back) {
            continue;
        }

        let len = lzma_memcmplen(buf, buf_back, 2, buf_avail);

        if len >= nice_len {
            *back_res = i as u32;
            *len_res = len;
            mf_skip(mf, len - 1);
            return;
        }

        if len > rep_len {
            rep_index = i as u32;
            rep_len = len;
        }
    }

    if len_main >= nice_len {
        *back_res = coder.matches[matches_count as usize - 1].dist + REPS as u32;
        *len_res = len_main;
        mf_skip(mf, len_main - 1);
        return;
    }

    let mut back_main = 0;
    if len_main >= 2 {
        back_main = coder.matches[matches_count as usize - 1].dist;

        while matches_count > 1 && len_main == coder.matches[matches_count as usize - 2].len + 1 {
            if !change_pair(coder.matches[matches_count as usize - 2].dist, back_main) {
                break;
            }

            matches_count -= 1;
            len_main = coder.matches[matches_count as usize - 1].len;
            back_main = coder.matches[matches_count as usize - 1].dist;
        }

        if len_main == 2 && back_main >= 0x80 {
            len_main = 1;
        }
    }

    if rep_len >= 2 {
        if rep_len + 1 >= len_main
            || (rep_len + 2 >= len_main && back_main > (1 << 9))
            || (rep_len + 3 >= len_main && back_main > (1 << 15))
        {
            *back_res = rep_index;
            *len_res = rep_len;
            mf_skip(mf, rep_len - 1);
            return;
        }
    }

    if len_main < 2 || buf_avail <= 2 {
        *back_res = u32::MAX;
        *len_res = 1;
        return;
    }

    coder.longest_match_length = MF_FIND(
        &mut mf.clone(),
        &mut coder.matches_count,
        &mut coder.matches,
    );

    if coder.longest_match_length >= 2 {
        let new_dist = coder.matches[coder.matches_count as usize - 1].dist;

        if (coder.longest_match_length >= len_main && new_dist < back_main)
            || (coder.longest_match_length == len_main + 1 && !change_pair(back_main, new_dist))
            || (coder.longest_match_length > len_main + 1)
            || (coder.longest_match_length + 1 >= len_main
                && len_main >= 3
                && change_pair(new_dist, back_main))
        {
            *back_res = u32::MAX;
            *len_res = 1;
            return;
        }
    }

    buf = &buf[1..];

    let limit = my_max(2, len_main - 1);

    for i in 0..REPS {
        if buf.starts_with(&buf[..buf.len() - coder.reps[i] as usize - 1]) {
            *back_res = u32::MAX;
            *len_res = 1;
            return;
        }
    }

    *back_res = back_main + REPS as u32;
    *len_res = len_main;
    mf_skip(mf, len_main - 2);
}
