/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    lz::LzmaMatch,
    rangecoder::{LzmaRangeEncoder, Probability},
};
use std::sync::{Arc, Mutex};

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
    pub choice: Arc<Mutex<u16>>,                                   // 选择概率
    pub choice2: Arc<Mutex<u16>>,                                  // 第二选择概率
    pub low: [[Arc<Mutex<u16>>; LEN_LOW_SYMBOLS]; POS_STATES_MAX], // 低位概率
    pub mid: [[Arc<Mutex<u16>>; LEN_MID_SYMBOLS]; POS_STATES_MAX], // 中位概率
    pub high: [Arc<Mutex<u16>>; LEN_HIGH_SYMBOLS],                 // 高位概率

    pub prices: [[u32; LEN_SYMBOLS]; POS_STATES_MAX], // 价格表
    pub table_size: u32,                              // 表大小
    pub counters: [u32; POS_STATES_MAX],              // 计数器
}
impl LzmaLengthEncoder {
    fn new() -> Self {
        LzmaLengthEncoder {
            choice: Arc::new(Mutex::new(Probability::default())), // 假设 Probability 实现了 Default
            choice2: Arc::new(Mutex::new(Probability::default())), // 假设 Probability 实现了 Default
            low: core::array::from_fn(|_| {
                core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default())))
            }), // 使用 core::array::from_fn 初始化低位概率数组
            mid: core::array::from_fn(|_| {
                core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default())))
            }),
            high: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))), // 使用默认概率初始化高位概率数组
            prices: [[0; LEN_SYMBOLS]; POS_STATES_MAX], // 初始化为零的价格表
            table_size: 0,                              // 默认值为 0
            counters: [0; POS_STATES_MAX],              // 初始化计数器数组为零
        }
    }
}

/// 最优结构体
#[derive(Debug, Clone, Copy)]
pub struct LzmaOptimal {
    pub state: u32, // LZMA 状态

    pub prev_1_is_literal: bool, // 前一个是否为字面值
    pub prev_2: bool,            // 前两个状态

    pub pos_prev_2: u32,  // 前两个位置
    pub back_prev_2: u32, // 前两个回退

    pub price: u32,     //
    pub pos_prev: u32,  // 前一个位置
    pub back_prev: u32, // 前一个回退

    pub backs: [u32; REPS], // 回退数组
}

impl Default for LzmaOptimal {
    fn default() -> Self {
        LzmaOptimal {
            state: 0, // 假设 LzmaLzmaState 实现了 Default
            prev_1_is_literal: false,
            prev_2: false,
            pos_prev_2: 0,
            back_prev_2: 0,
            price: 0,
            pos_prev: 0,
            back_prev: 0,
            backs: [0; REPS], // 假设 REPS 是已定义的常量，初始化为零的回退数组
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
    pub uncomp_size_ptr: Option<u64>, // 有可能类型不正确

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

    pub pos_mask: u32,             // (1 << pos_bits) - 1
    pub literal_context_bits: u32, // 字面值上下文位
    pub literal_pos_mask: u32,     // 字面值位置掩码

    // 这些与 lzma_decoder.c 中相同。请参阅那里的注释。
    pub literal: Box<[[Arc<Mutex<Probability>>; LITERAL_CODER_SIZE]; LITERAL_CODERS_MAX]>,
    pub is_match: [[Arc<Mutex<Probability>>; POS_STATES_MAX]; STATES],
    pub is_rep: [Arc<Mutex<Probability>>; STATES],
    pub is_rep0: [Arc<Mutex<Probability>>; STATES],
    pub is_rep1: [Arc<Mutex<Probability>>; STATES],
    pub is_rep2: [Arc<Mutex<Probability>>; STATES],
    pub is_rep0_long: [[Arc<Mutex<Probability>>; POS_STATES_MAX]; STATES],
    pub dist_slot: [[Arc<Mutex<Probability>>; DIST_SLOTS]; DIST_STATES],
    pub dist_special: [Arc<Mutex<Probability>>; FULL_DISTANCES - DIST_MODEL_END],
    pub dist_align: [Arc<Mutex<Probability>>; ALIGN_SIZE],

    // 这些与 lzma_decoder.c 中相同，但编码器还包括价格表。
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

// impl Default for LzmaLzma1Encoder {
//     fn default() -> Self {
//         LzmaLzma1Encoder {
//             rc: LzmaRangeEncoder::default(), // 假设 LzmaRangeEncoder 实现了 Default
//             uncomp_size: 0,
//             out_limit: 0,
//             uncomp_size_ptr: None, // Option 类型默认是 None
//             state: 0,              // 假设 LzmaLzmaState 实现了 Default
//             reps: [0; REPS],
//             matches: [LzmaMatch::default(); MATCH_LEN_MAX + 1], // 假设 LzmaMatch 实现了 Default
//             matches_count: 0,
//             longest_match_length: 0,
//             fast_mode: false,
//             is_initialized: false,
//             is_flushed: false,
//             use_eopm: false,
//             pos_mask: 0,
//             literal_context_bits: 0,
//             literal_pos_mask: 0,
//             literal: [[Probability::default(); LITERAL_CODER_SIZE]; LITERAL_CODERS_MAX], // 假设 Probability 实现了 Default
//             is_match: [[Probability::default(); POS_STATES_MAX]; STATES],
//             is_rep: [Probability::default(); STATES],
//             is_rep0: [Probability::default(); STATES],
//             is_rep1: [Probability::default(); STATES],
//             is_rep2: [Probability::default(); STATES],
//             is_rep0_long: [[Probability::default(); POS_STATES_MAX]; STATES],
//             dist_slot: [[Probability::default(); DIST_SLOTS]; DIST_STATES],
//             dist_special: [Probability::default(); FULL_DISTANCES - DIST_MODEL_END],
//             dist_align: [Probability::default(); ALIGN_SIZE],
//             match_len_encoder: LzmaLengthEncoder::default(), // 假设 LzmaLengthEncoder 实现了 Default
//             rep_len_encoder: LzmaLengthEncoder::default(), // 假设 LzmaLengthEncoder 实现了 Default
//             dist_slot_prices: [[0; DIST_SLOTS]; DIST_STATES],
//             dist_prices: [[0; FULL_DISTANCES]; DIST_STATES],
//             dist_table_size: 0,
//             match_price_count: 0,
//             align_prices: [0; ALIGN_SIZE],
//             align_price_count: 0,
//             opts_end_index: 0,
//             opts_current_index: 0,
//             opts: [LzmaOptimal::default(); OPTS], // 假设 LzmaOptimal 实现了 Default
//         }
//     }
// }

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
            literal: Box::new(core::array::from_fn(|_| {
                core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default())))
            })),
            is_match: core::array::from_fn(|_| {
                core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default())))
            }),
            is_rep: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
            is_rep0: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
            is_rep1: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
            is_rep2: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
            is_rep0_long: core::array::from_fn(|_| {
                core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default())))
            }),
            dist_slot: core::array::from_fn(|_| {
                core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default())))
            }),
            dist_special: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
            dist_align: core::array::from_fn(|_| Arc::new(Mutex::new(Probability::default()))),
            match_len_encoder: LzmaLengthEncoder::new(),
            rep_len_encoder: LzmaLengthEncoder::new(),
            dist_slot_prices: core::array::from_fn(|_| [0; DIST_SLOTS]),
            dist_prices: core::array::from_fn(|_| [0; FULL_DISTANCES]),
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
