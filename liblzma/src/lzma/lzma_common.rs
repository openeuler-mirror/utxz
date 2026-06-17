/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::rangecoder::bit_reset;
use crate::{
    api::{LzmaOptionsLzma, LZMA_LCLP_MAX, LZMA_PB_MAX},
    rangecoder::Probability,
};

/// LzmaLzmaState 各状态对应的 const 常量（转换自枚举）
pub const STATE_LIT_LIT: u32 = 0;
pub const STATE_MATCH_LIT_LIT: u32 = 1;
pub const STATE_REP_LIT_LIT: u32 = 2;
pub const STATE_SHORTREP_LIT_LIT: u32 = 3;
pub const STATE_MATCH_LIT: u32 = 4;
pub const STATE_REP_LIT: u32 = 5;
pub const STATE_SHORTREP_LIT: u32 = 6;
pub const STATE_LIT_MATCH: u32 = 7;
pub const STATE_LIT_LONGREP: u32 = 8;
pub const STATE_LIT_SHORTREP: u32 = 9;
pub const STATE_NONLIT_MATCH: u32 = 10;
pub const STATE_NONLIT_REP: u32 = 11;

// #[derive(Clone, Debug, PartialEq, TryFromPrimitive, PartialOrd, Copy, Default)]
// #[repr(u32)]
// pub enum LzmaLzmaState {
//     #[default]
//     LitLit,
//     MatchLitLit,
//     RepLitLit,
//     ShortRepLitLit,
//     MatchLit,
//     RepLit,
//     ShortRepLit,
//     LitMatch,
//     LitLongRep,
//     LitShortRep,
//     NonLitMatch,
//     NonLitRep,
// }

pub fn is_lclppb_valid(options: &LzmaOptionsLzma) -> bool {
    options.lc <= LZMA_LCLP_MAX
        && options.lp <= LZMA_LCLP_MAX
        && options.lc + options.lp <= LZMA_LCLP_MAX
        && options.pb <= LZMA_PB_MAX
}

pub const LITERAL_CODERS_MAX: usize = 1 << LZMA_LCLP_MAX;
pub const LITERAL_CODER_SIZE: usize = 0x300;
pub const STATES: usize = 12;
pub const POS_STATES_MAX: usize = 1 << LZMA_PB_MAX;
pub const DIST_STATES: usize = 4;
pub const DIST_SLOT_BITS: usize = 6;
pub const DIST_SLOTS: usize = 1 << DIST_SLOT_BITS;
pub const DIST_MODEL_END: usize = 14;
pub const FULL_DISTANCES_BITS: usize = DIST_MODEL_END / 2;
pub const FULL_DISTANCES: usize = 1 << FULL_DISTANCES_BITS;
pub const ALIGN_BITS: usize = 4;
pub const ALIGN_SIZE: usize = 1 << ALIGN_BITS;
pub const ALIGN_MASK: usize = ALIGN_SIZE - 1;

pub const LEN_LOW_BITS: usize = 3;
pub const LEN_LOW_SYMBOLS: usize = 1 << LEN_LOW_BITS;
pub const LEN_MID_BITS: usize = 3;
pub const LEN_MID_SYMBOLS: usize = 1 << LEN_MID_BITS;
pub const LEN_HIGH_BITS: usize = 8;
pub const LEN_HIGH_SYMBOLS: usize = 1 << LEN_HIGH_BITS;
pub const LEN_SYMBOLS: usize = LEN_LOW_SYMBOLS + LEN_MID_SYMBOLS + LEN_HIGH_SYMBOLS;

pub const MATCH_LEN_MIN: usize = 2;
pub const MATCH_LEN_MAX: usize = MATCH_LEN_MIN + LEN_SYMBOLS - 1;

pub const REPS: usize = 4;

// pub const LIT_STATES: usize = 7;

// #[macro_export]
// macro_rules! update_literal {
//     ($state:expr) => {
//         $state = if $state <= LzmaLzmaState::ShortRepLitLit {
//             LzmaLzmaState::LitLit
//         } else if $state <= LzmaLzmaState::LitShortRep {
//             $state.subtract(3).unwrap_or(LzmaLzmaState::LitLit)
//         } else {
//             $state.subtract(6).unwrap_or(LzmaLzmaState::LitLit)
//         };
//     };
// }

/// 更新状态为 literal 状态
pub fn update_literal(mut state: u32) -> u32 {
    if state <= STATE_SHORTREP_LIT_LIT {
        state = STATE_LIT_LIT;
    } else if state <= STATE_LIT_SHORTREP {
        state = state - 3;
    } else {
        state = state - 6;
    }
    state
}

pub fn update_match(state: &mut u32) {
    *state = if *state < LIT_STATES.try_into().unwrap() {
        STATE_LIT_MATCH
    } else {
        STATE_NONLIT_MATCH
    };
}
/// 更新长重复状态，根据当前 state 与 LIT_STATES 的比较结果更新状态值
pub fn update_long_rep(state: &mut u32) {
    *state = if *state < LIT_STATES as u32 {
        STATE_LIT_LONGREP
    } else {
        STATE_NONLIT_REP
    };
}
/// 更新短重复状态，根据当前 state 与 LIT_STATES 的比较结果更新状态值
pub fn update_short_rep(state: &mut u32) {
    *state = if *state < LIT_STATES as u32 {
        STATE_LIT_SHORTREP
    } else {
        STATE_NONLIT_REP
    };
}

pub const LIT_STATES: usize = 7;
/// 判断状态是否为 literal 状态
pub fn is_literal_state(state: u32) -> bool {
    state < LIT_STATES as u32
}

// #[macro_export]
// macro_rules! literal_subcoder {
//     ($probs:expr, $lc:expr, $lp_mask:expr, $pos:expr, $prev_byte:expr) => {
//         $probs[((($pos) & ($lp_mask)) << ($lc)) + (($prev_byte as u32) >> (8 - ($lc)))]
//     };
// }

pub fn literal_init(probs: &mut [[u16; LITERAL_CODER_SIZE]], lc: u32, lp: u32) {
    assert!(lc + lp <= LZMA_LCLP_MAX);

    let coders = 1 << (lc + lp);

    for i in 0..coders {
        for j in 0..LITERAL_CODER_SIZE {
            bit_reset(&mut probs[i][j]);
        }
    }
}

#[macro_export]
macro_rules! get_dist_state {
    ($len:expr) => {
        if $len < DIST_STATES as u32 + MATCH_LEN_MIN as u32 {
            $len - MATCH_LEN_MIN as u32
        } else {
            DIST_STATES as u32 - 1
        }
    };
}

pub const DIST_MODEL_START: usize = 4;
