/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::lzma::{FULL_DISTANCES, FULL_DISTANCES_BITS, LZMA_FASTPOS};

pub const FASTPOS_BITS: usize = 13;

// pub static LZMA_FASTPOS: [u8; 1 << FASTPOS_BITS] = [0; 1 << FASTPOS_BITS]; // 假设已初始化

pub fn fastpos_shift(extra: u32, n: u32) -> u32 {
    extra + n * (FASTPOS_BITS as u32 - 1)
}

pub fn fastpos_limit(extra: u32, n: u32) -> u32 {
    1 << (FASTPOS_BITS as u32 + fastpos_shift(extra, n))
}

pub fn fastpos_result(dist: u32, extra: u32, n: u32) -> u32 {
    LZMA_FASTPOS[(dist >> fastpos_shift(extra, n)) as usize] as u32 + 2 * fastpos_shift(extra, n)
}
pub fn get_dist_slot(dist: u32) -> u32 {
    let mut ret: u32 = 0;
    // 如果距离足够小，可以直接从预计算表中获取结果。
    if dist < fastpos_limit(0, 0) {
        ret = LZMA_FASTPOS[dist as usize] as u32;
        return ret;
    }

    if dist < fastpos_limit(0, 1) {
        ret = fastpos_result(dist, 0, 1);
        return ret;
    }

    ret = fastpos_result(dist, 0, 2);
    ret
}

pub fn get_dist_slot_2(dist: u32) -> u32 {
    assert!(dist >= FULL_DISTANCES as u32);

    if dist < fastpos_limit(FULL_DISTANCES_BITS as u32 - 1, 0) {
        return fastpos_result(dist, FULL_DISTANCES_BITS as u32 - 1, 0);
    }

    if dist < fastpos_limit(FULL_DISTANCES_BITS as u32 - 1, 1) {
        return fastpos_result(dist, FULL_DISTANCES_BITS as u32 - 1, 1);
    }

    fastpos_result(dist, FULL_DISTANCES_BITS as u32 - 1, 2)
}
