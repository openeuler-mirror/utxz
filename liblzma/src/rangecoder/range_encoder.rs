/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use num_enum::TryFromPrimitive;

use crate::{
    lzma::{LITERAL_CODERS_MAX, LITERAL_CODER_SIZE},
    rangecoder::{RC_BIT_MODEL_TOTAL, RC_BIT_MODEL_TOTAL_BITS, RC_MOVE_BITS, RC_TOP_VALUE},
};

use super::{Probability, RC_SHIFT_BITS};

#[derive(Debug, Clone)]
pub struct LzmaRangeEncoder {
    pub low: u64,
    pub cache_size: u64,
    pub range: u32,
    pub cache: u8,

    /// Number of bytes written out
    pub out_total: u64,

    /// Number of symbols pending (for rc_direct batch)
    pub count: usize,
    pub pos: usize,

    /// Symbols to encode (used by rc_direct)
    pub symbols: [RcSymbol; RC_SYMBOLS_MAX],
}

const RC_SYMBOLS_MAX: usize = 53;

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

    /// Encode a single bit immediately using the given probability value.
    /// Returns true if the output buffer became full.
    pub fn rc_bit(
        &mut self,
        prob: &mut u16,
        bit: u32,
        out: &mut [u8],
        out_pos: &mut usize,
        out_size: usize,
    ) -> bool {
        if self.range < RC_TOP_VALUE {
            if self.rc_shift_low(out, out_pos, out_size) {
                return true;
            }
            self.range <<= RC_SHIFT_BITS;
        }

        if bit == 0 {
            self.range = (self.range >> RC_BIT_MODEL_TOTAL_BITS) * (*prob as u32);
            *prob += (RC_BIT_MODEL_TOTAL as u16 - *prob) >> RC_MOVE_BITS;
        } else {
            let bound = (*prob as u32) * (self.range >> RC_BIT_MODEL_TOTAL_BITS);
            self.low += bound as u64;
            self.range -= bound;
            *prob -= *prob >> RC_MOVE_BITS;
        }

        false
    }

    /// Encode a bit tree immediately. Returns true if output buffer became full.
    pub fn rc_bittree(
        &mut self,
        probs: &mut [u16],
        mut bit_count: u32,
        symbol: u32,
        out: &mut [u8],
        out_pos: &mut usize,
        out_size: usize,
    ) -> bool {
        let mut model_index = 1;

        while bit_count != 0 {
            bit_count -= 1;
            let bit = (symbol >> bit_count) & 1;
            if self.rc_bit(&mut probs[model_index], bit, out, out_pos, out_size) {
                return true;
            }
            model_index = (model_index << 1) + bit as usize;
        }

        false
    }

    /// Encode a bit tree in reverse order. Returns true if output buffer became full.
    pub fn rc_bittree_reverse(
        &mut self,
        probs: &mut [u16],
        mut bit_count: u32,
        mut symbol: u32,
        flags: u8,
        out: &mut [u8],
        out_pos: &mut usize,
        out_size: usize,
    ) -> bool {
        let mut model_index = 1;

        if flags != 0 {
            let bit = symbol & 1;
            symbol >>= 1;
            if self.rc_bit(&mut probs[0], bit, out, out_pos, out_size) {
                return true;
            }
            model_index = (model_index << 1) + bit as usize;
            bit_count -= 1;
        }

        while bit_count != 0 {
            let bit = symbol & 1;
            symbol >>= 1;
            if self.rc_bit(&mut probs[model_index], bit, out, out_pos, out_size) {
                return true;
            }
            model_index = (model_index << 1) + bit as usize;
            bit_count -= 1;
        }

        false
    }

    /// Encode direct bits. Returns true if output buffer became full.
    pub fn rc_direct(
        &mut self,
        mut value: u32,
        mut bit_count: u32,
        out: &mut [u8],
        out_pos: &mut usize,
        out_size: usize,
    ) -> bool {
        while bit_count != 0 {
            bit_count -= 1;
            if self.range < RC_TOP_VALUE {
                if self.rc_shift_low(out, out_pos, out_size) {
                    return true;
                }
                self.range <<= RC_SHIFT_BITS;
            }

            self.range >>= 1;
            if ((value >> bit_count) & 1) != 0 {
                self.low += self.range as u64;
            }
        }

        false
    }

    /// Flush the range encoder. Returns true if output buffer became full.
    pub fn rc_flush(&mut self, out: &mut [u8], out_pos: &mut usize, out_size: usize) -> bool {
        self.range = u32::MAX;
        for _ in 0..5 {
            if self.rc_shift_low(out, out_pos, out_size) {
                return true;
            }
        }
        false
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

    /// Compatibility method — no-op since encoding is now immediate.
    /// Returns false (output not full) for backward compatibility.
    pub fn rc_encode(&mut self, _out: &mut [u8], _out_pos: &mut usize, _out_size: usize) -> bool {
        false
    }
}

fn rc_shift_low_dummy(
    low: &mut u64,
    cache_size: &mut u64,
    cache: &mut u8,
    out_pos: &mut u64,
    out_size: u64,
) -> bool {
    if (*low as u32) < 0xFF000000 || (*low >> 32) as u32 != 0 {
        while *cache_size != 0 {
            if *out_pos == out_size {
                return true;
            }

            *out_pos += 1;
            *cache = 0xFF;
            *cache_size -= 1;
        }

        *cache = (*low >> 24) as u8;
    }

    *cache_size += 1;
    *low = (*low & 0x00FFFFFF) << RC_SHIFT_BITS;

    false
}

/// Dummy encoding — simulates encoding to estimate output size.
/// With immediate encoding, we use out_total for limit checking instead.
pub fn rc_encode_dummy(rc: &LzmaRangeEncoder, out_limit: u64) -> bool {
    rc.out_total >= out_limit
}

pub fn rc_pending(rc: &LzmaRangeEncoder) -> u64 {
    rc.cache_size + 5 - 1
}

/// Free function wrapper for rc_bit
pub fn rc_bit(
    rc: &mut LzmaRangeEncoder,
    prob: &mut u16,
    bit: u32,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> bool {
    rc.rc_bit(prob, bit, out, out_pos, out_size)
}
