/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use num_enum::TryFromPrimitive;
use std::sync::{Arc, Mutex};

use crate::{
    //  lzma::{LITERAL_CODERS_MAX, LITERAL_CODER_SIZE},
    rangecoder::{RC_BIT_MODEL_TOTAL, RC_BIT_MODEL_TOTAL_BITS, RC_MOVE_BITS, RC_TOP_VALUE},
};

use super::{Probability, RC_SHIFT_BITS};

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

    pub fn rc_bittree(&mut self, probs: &mut [Arc<Mutex<u16>>], mut bit_count: u32, symbol: u32) {
        let mut model_index = 1;

        while bit_count != 0 {
            bit_count -= 1;
            let bit = (symbol >> bit_count) & 1;
            self.rc_bit(Arc::clone(&probs[model_index]), bit);
            model_index = (model_index << 1) + bit as usize;
        }
    }

    pub fn rc_bittree_reverse(
        &mut self,
        probs: &mut [Arc<Mutex<u16>>],
        mut bit_count: u32,
        mut symbol: u32,
        flags: u8,
    ) {
        let mut model_index = 1;

        if flags != 0 {
            let bit = symbol & 1;
            symbol >>= 1;
            self.rc_bit(Arc::clone(&probs[0]), bit);
            model_index = (model_index << 1) + bit as usize;
            bit_count -= 1;
        }

        while bit_count != 0 {
            let bit = symbol & 1;
            symbol >>= 1;
            self.rc_bit(Arc::clone(&probs[model_index]), bit);
            model_index = (model_index << 1) + bit as usize;
            bit_count -= 1;
        }
    }

    pub fn rc_direct(&mut self, value: u32, mut bit_count: u32) {
        while bit_count != 0 {
            bit_count -= 1;
            let shifted_value = value >> bit_count;
            let bit = shifted_value & 1;
            let symbol = if bit == 0 {
                RcSymbol::RcDirect0
            } else {
                RcSymbol::RcDirect1
            };
            self.symbols[self.count] = symbol;
            self.count += 1;
        }
    }

    pub fn rc_flush(&mut self) {
        for _ in 0..5 {
            self.symbols[self.count] = RcSymbol::RcFlush;
            self.count += 1;
        }
    }

    pub fn rc_shift_low(&mut self, out: &mut [u8], out_pos: &mut usize, out_size: usize) -> bool {
        if (self.low as u32) < 0xFF000000 || (self.low >> 32) as u32 != 0 {
            while self.cache_size != 0 {
                if *out_pos == out_size {
                    return true;
                }

                out[*out_pos] = self.cache.wrapping_add((self.low >> 32) as u8);
                *out_pos += 1;
                self.out_total += 1;
                self.cache = 0xFF;
                self.cache_size -= 1;
            }

            self.cache = (self.low >> 24) as u8;
        }

        self.cache_size += 1;
        self.low = (self.low & 0x00FFFFFF) << RC_SHIFT_BITS;

        false
    }

    pub fn rc_encode(&mut self, out: &mut [u8], out_pos: &mut usize, out_size: usize) -> bool {
        assert!(self.count <= RC_SYMBOLS_MAX);

        while self.pos < self.count {
            if self.range < RC_TOP_VALUE {
                if self.rc_shift_low(out, out_pos, out_size) {
                    return true;
                }

                self.range <<= RC_SHIFT_BITS;
            }

            match self.symbols[self.pos] {
                RcSymbol::RcBit0 => {
                    let mut prob = *self.probs[self.pos].lock().unwrap();
                    self.range = (self.range >> RC_BIT_MODEL_TOTAL_BITS) * prob as u32;
                    prob += (RC_BIT_MODEL_TOTAL as u16 - prob) >> RC_MOVE_BITS;
                    *self.probs[self.pos].lock().unwrap() = prob;
                }
                RcSymbol::RcBit1 => {
                    let mut prob: u16 = *self.probs[self.pos].lock().unwrap();
                    let bound: u32 = prob as u32 * (self.range >> RC_BIT_MODEL_TOTAL_BITS);
                    self.low += bound as u64;
                    self.range -= bound;
                    prob -= prob >> RC_MOVE_BITS;
                    *self.probs[self.pos].lock().unwrap() = prob;
                }
                RcSymbol::RcDirect0 => {
                    self.range >>= 1;
                }
                RcSymbol::RcDirect1 => {
                    self.range >>= 1;
                    self.low += self.range as u64;
                }
                RcSymbol::RcFlush => {
                    self.range = u32::MAX;

                    while self.pos < self.count {
                        if self.rc_shift_low(out, out_pos, out_size) {
                            return true;
                        }
                        self.pos += 1;
                    }

                    self.rc_reset();
                    return false;
                }
            }

            self.pos += 1;
        }

        self.count = 0;
        self.pos = 0;

        false
    }
}
