/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

pub const RC_SHIFT_BITS: u32 = 8;
pub const RC_TOP_BITS: u32 = 24;
pub const RC_TOP_VALUE: u32 = 1 << RC_TOP_BITS;
pub const RC_BIT_MODEL_TOTAL_BITS: u32 = 11;
pub const RC_BIT_MODEL_TOTAL: u32 = 1 << RC_BIT_MODEL_TOTAL_BITS;
pub const RC_MOVE_BITS: u32 = 5;

#[macro_export]
macro_rules! bit_reset {
    ($prob:expr) => {
        $prob = (1u16 << 11) >> 1;
    };
}

#[macro_export]
macro_rules! bittree_reset {
    ($probs:expr, $bit_levels:expr) => {
        for bt_i in 0..(1 << $bit_levels) {
            bit_reset!($probs[bt_i]);
        }
    };
}

pub fn bit_reset(prob: &mut u16) {
    *prob = (1u16 << 11) >> 1;
}

pub fn bittree_reset(probs: &mut [u16], bit_levels: usize) {
    for bt_i in 0..(1 << bit_levels) {
        bit_reset(&mut probs[bt_i]);
    }
}

pub type Probability = u16;
