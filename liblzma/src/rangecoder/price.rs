/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::{Probability, RC_BIT_MODEL_TOTAL};
use crate::rangecoder::LZMA_RC_PRICES;
use std::sync::{Arc, Mutex};

pub const RC_MOVE_REDUCING_BITS: u32 = 4;
pub const RC_BIT_PRICE_SHIFT_BITS: u32 = 4;
pub const RC_PRICE_TABLE_SIZE: usize = (RC_BIT_MODEL_TOTAL >> RC_MOVE_REDUCING_BITS) as usize;

pub const RC_INFINITY_PRICE: u32 = 1 << 30;

/// 查找表，用于计算价格
// pub static LZMA_RC_PRICES: [u8; RC_PRICE_TABLE_SIZE] = [0; RC_PRICE_TABLE_SIZE]; // 假设已初始化

pub fn rc_bit_price(prob: Probability, bit: u32) -> u32 {
    LZMA_RC_PRICES[((prob ^ ((0u32.wrapping_sub(bit) as u16) & (RC_BIT_MODEL_TOTAL as u16 - 1)))
        >> RC_MOVE_REDUCING_BITS as u16) as usize] as u32
}

pub fn rc_bit_0_price(prob: Probability) -> u32 {
    LZMA_RC_PRICES[(prob >> RC_MOVE_REDUCING_BITS) as usize] as u32
}

pub fn rc_bit_1_price(prob: Probability) -> u32 {
    LZMA_RC_PRICES[((prob ^ (RC_BIT_MODEL_TOTAL - 1) as u16) >> RC_MOVE_REDUCING_BITS) as usize]
        as u32
}

pub fn rc_bittree_price(probs: &[Arc<Mutex<u16>>], bit_levels: u32, mut symbol: u32) -> u32 {
    let mut price = 0;
    symbol += 1 << bit_levels;

    while symbol != 1 {
        let bit = symbol & 1;
        symbol >>= 1;
        let prob = *probs[symbol as usize].lock().unwrap();
        price += rc_bit_price(prob, bit);
    }

    price
}

// 修改rc_bittree_reverse_price添加flags参数
pub fn rc_bittree_reverse_price(
    probs: &[Arc<Mutex<Probability>>],
    mut bit_levels: u32,
    mut symbol: u32,
    flags: u8,
) -> u32 {
    let mut price = 0;
    let mut model_index = 1;

    if flags != 0 {
        let bit = symbol & 1;
        symbol >>= 1;
        price += rc_bit_price(*probs[0].lock().unwrap(), bit);
        model_index = (model_index << 1) + bit as usize;
        bit_levels -= 1;
    }

    while bit_levels != 0 {
        let bit = symbol & 1;
        symbol >>= 1;
        price += rc_bit_price(*probs[model_index].lock().unwrap(), bit);
        model_index = (model_index << 1) + bit as usize;
        bit_levels -= 1;
    }

    price
}

pub fn rc_direct_price(bits: u32) -> u32 {
    bits << RC_BIT_PRICE_SHIFT_BITS
}
