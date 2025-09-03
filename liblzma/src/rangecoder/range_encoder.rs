/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use num_enum::TryFromPrimitive;
use std::sync::{Arc, Mutex};

// use crate::rangecoder::{RC_BIT_MODEL_TOTAL, RC_BIT_MODEL_TOTAL_BITS, RC_MOVE_BITS, RC_TOP_VALUE};

use super::Probability;

const RC_SYMBOLS_MAX: usize = 53;

#[derive(Debug, Clone)]
pub struct LzmaRangeEncoder {
    pub low: u64,
    pub cache_size: u64,
    pub range: u32,
    pub cache: u8,

    /// Number of bytes written out by rc_encode() -> rc_shift_low()
    pub out_total: u64,

    /// Number of symbols in the tables
    pub count: usize,

    /// rc_encode()'s position in the tables
    pub pos: usize,

    /// Symbols to encode
    pub symbols: [RcSymbol; RC_SYMBOLS_MAX],

    /// Probabilities associated with RC_BIT_0 or RC_BIT_1
    pub probs: [Arc<Mutex<Probability>>; RC_SYMBOLS_MAX],
}

// impl Default for LzmaRangeEncoder {
//     fn default() -> Self {
//         LzmaRangeEncoder {
//             low: 0,
//             cache_size: 0,
//             range: 0,
//             cache: 0,
//             out_total: 0,
//             count: 0,
//             pos: 0,
//             symbols: [RcSymbol::default(); RC_SYMBOLS_MAX], // 使用 RcSymbol 的默认值初始化
//             probs: [Probability::default(); RC_SYMBOLS_MAX], // 使用 Probability 的默认值初始化
//         }
//     }
// }

#[derive(Clone, Debug, PartialEq, TryFromPrimitive, Default, Copy)]
#[repr(u32)]
pub enum RcSymbol {
    #[default]
    RcBit0 = 0,
    RcBit1 = 1,
    RcDirect0 = 2,
    RcDirect1 = 3,
    RcFlush = 4,
}

impl LzmaRangeEncoder {
    pub fn new() -> Self {
        LzmaRangeEncoder {
            low: 0,
            cache_size: 0,
            range: u32::MAX,
            cache: 0,
            out_total: 0,
            count: 0,
            pos: 0,
            symbols: [RcSymbol::default(); RC_SYMBOLS_MAX],
            probs: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
        }
    }

    pub fn rc_reset(&mut self) {
        self.low = 0;
        self.cache_size = 1;
        self.range = u32::MAX;
        self.cache = 0;
        self.out_total = 0;
        self.count = 0;
        self.pos = 0;
    }

    pub fn rc_forget(&mut self) {
        assert!(self.pos == 0);
        self.count = 0;
    }

    pub fn rc_bit(&mut self, prob: Arc<Mutex<u16>>, bit: u32) {
        self.symbols[self.count] = RcSymbol::try_from(bit).unwrap(); // Assuming bit is 0 or 1
        self.probs[self.count] = prob;
        self.count += 1;
    }
}
