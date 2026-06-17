/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    lz::LzmaMatch,
    rangecoder::{LzmaRangeEncoder, Probability},
};

use super::{
    ALIGN_SIZE, DIST_MODEL_END, DIST_SLOTS, DIST_STATES, FULL_DISTANCES, LEN_HIGH_SYMBOLS,
    LEN_LOW_SYMBOLS, LEN_MID_SYMBOLS, LEN_SYMBOLS, LITERAL_CODERS_MAX, LITERAL_CODER_SIZE,
    MATCH_LEN_MAX, POS_STATES_MAX, REPS, STATES,
};

/// 选项常量
pub const OPTS: usize = 1 << 12;

/// 长度编码器结构体
#[derive(Debug, Clone)]
pub struct LzmaLengthEncoder {
    pub choice: Probability,
    pub choice2: Probability,
    pub low: [[Probability; LEN_LOW_SYMBOLS]; POS_STATES_MAX],
    pub mid: [[Probability; LEN_MID_SYMBOLS]; POS_STATES_MAX],
    pub high: [Probability; LEN_HIGH_SYMBOLS],

    pub prices: [[u32; LEN_SYMBOLS]; POS_STATES_MAX],
    pub table_size: u32,
    pub counters: [u32; POS_STATES_MAX],
}
impl LzmaLengthEncoder {
    pub fn new() -> Self {
        LzmaLengthEncoder {
            choice: Probability::default(),
            choice2: Probability::default(),
            low: [[Probability::default(); LEN_LOW_SYMBOLS]; POS_STATES_MAX],
            mid: [[Probability::default(); LEN_MID_SYMBOLS]; POS_STATES_MAX],
            high: [Probability::default(); LEN_HIGH_SYMBOLS],
            prices: [[0; LEN_SYMBOLS]; POS_STATES_MAX],
            table_size: 0,
            counters: [0; POS_STATES_MAX],
        }
    }
}

/// 最优结构体
#[derive(Debug, Clone, Copy)]
pub struct LzmaOptimal {
    pub state: u32,

    pub prev_1_is_literal: bool,
    pub prev_2: bool,

    pub pos_prev_2: u32,
    pub back_prev_2: u32,

    pub price: u32,
    pub pos_prev: u32,
    pub back_prev: u32,

    pub backs: [u32; REPS],
}

impl Default for LzmaOptimal {
    fn default() -> Self {
        LzmaOptimal {
            state: 0,
            prev_1_is_literal: false,
            prev_2: false,
            pos_prev_2: 0,
            back_prev_2: 0,
            price: 0,
            pos_prev: 0,
            back_prev: 0,
            backs: [0; REPS],
        }
    }
}

/// LZMA1 编码器结构体
#[derive(Debug, Clone)]
pub struct LzmaLzma1Encoder {
    /// 范围编码器
    pub rc: LzmaRangeEncoder,

    /// 未压缩大小（不包括可能的预设字典）
    pub uncomp_size: u64,

    /// 如果非零，最多生成此大小的输出。
    /// 可能会有一些输入缺失。
    pub out_limit: u64,

    /// 如果上面的 out_limit 非零，*uncomp_size_ptr 被设置为
    /// 我们能够放入输出缓冲区的未压缩数据量。
    pub uncomp_size_ptr: Option<u64>,

    /// 状态
    pub state: u32,

    /// 最近的四个匹配距离
    pub reps: [u32; REPS],

    /// 匹配候选数组
    pub matches: [LzmaMatch; MATCH_LEN_MAX + 1],

    /// matches[] 中的匹配候选数量
    pub matches_count: u32,

    /// 用于保存最长匹配长度的变量
    pub longest_match_length: u32,

    /// 如果使用 getoptimumfast 则为真
    pub fast_mode: bool,

    /// 如果编码器已通过编码第一个字节作为字面值进行初始化，则为真。
    pub is_initialized: bool,

    /// 如果范围编码器已被刷新，但尚未将所有字节写入输出缓冲区，则为真。
    pub is_flushed: bool,

    /// 如果将写入有效载荷结束标记，则为真。
    pub use_eopm: bool,

    pub pos_mask: u32,
    pub literal_context_bits: u32,
    pub literal_pos_mask: u32,

    pub literal: Box<[[Probability; LITERAL_CODER_SIZE]; LITERAL_CODERS_MAX]>,
    pub is_match: [[Probability; POS_STATES_MAX]; STATES],
    pub is_rep: [Probability; STATES],
    pub is_rep0: [Probability; STATES],
    pub is_rep1: [Probability; STATES],
    pub is_rep2: [Probability; STATES],
    pub is_rep0_long: [[Probability; POS_STATES_MAX]; STATES],
    pub dist_slot: [[Probability; DIST_SLOTS]; DIST_STATES],
    pub dist_special: [Probability; FULL_DISTANCES - DIST_MODEL_END],
    pub dist_align: [Probability; ALIGN_SIZE],

    pub match_len_encoder: LzmaLengthEncoder,
    pub rep_len_encoder: LzmaLengthEncoder,

    // 价格表
    pub dist_slot_prices: [[u32; DIST_SLOTS]; DIST_STATES],
    pub dist_prices: [[u32; FULL_DISTANCES]; DIST_STATES],
    pub dist_table_size: u32,
    pub match_price_count: u32,

    pub align_prices: [u32; ALIGN_SIZE],
    pub align_price_count: u32,

    // 最优
    pub opts_end_index: u32,
    pub opts_current_index: u32,
    pub opts: [LzmaOptimal; OPTS],
}

impl LzmaLzma1Encoder {
    pub fn new() -> Self {
        LzmaLzma1Encoder {
            rc: LzmaRangeEncoder::new(),
            uncomp_size: 0,
            out_limit: 0,
            uncomp_size_ptr: None,
            state: 0,
            reps: [0; REPS],
            matches: [LzmaMatch::default(); MATCH_LEN_MAX + 1],
            matches_count: 0,
            longest_match_length: 0,
            fast_mode: false,
            is_initialized: false,
            is_flushed: false,
            use_eopm: false,
            pos_mask: 0,
            literal_context_bits: 0,
            literal_pos_mask: 0,
            literal: Box::new([[Probability::default(); LITERAL_CODER_SIZE]; LITERAL_CODERS_MAX]),
            is_match: [[Probability::default(); POS_STATES_MAX]; STATES],
            is_rep: [Probability::default(); STATES],
            is_rep0: [Probability::default(); STATES],
            is_rep1: [Probability::default(); STATES],
            is_rep2: [Probability::default(); STATES],
            is_rep0_long: [[Probability::default(); POS_STATES_MAX]; STATES],
            dist_slot: [[Probability::default(); DIST_SLOTS]; DIST_STATES],
            dist_special: [Probability::default(); FULL_DISTANCES - DIST_MODEL_END],
            dist_align: [Probability::default(); ALIGN_SIZE],
            match_len_encoder: LzmaLengthEncoder::new(),
            rep_len_encoder: LzmaLengthEncoder::new(),
            dist_slot_prices: [[0; DIST_SLOTS]; DIST_STATES],
            dist_prices: [[0; FULL_DISTANCES]; DIST_STATES],
            dist_table_size: 0,
            match_price_count: 0,
            align_prices: [0; ALIGN_SIZE],
            align_price_count: 0,
            opts_end_index: 0,
            opts_current_index: 0,
            opts: core::array::from_fn(|_| LzmaOptimal::default()),
        }
    }
}
