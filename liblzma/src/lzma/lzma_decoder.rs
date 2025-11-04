/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::LzmaVli,
    lzma::{
        STATE_LIT_LIT, STATE_MATCH_LIT, STATE_MATCH_LIT_LIT, STATE_REP_LIT, STATE_REP_LIT_LIT,
        STATE_SHORTREP_LIT, STATE_SHORTREP_LIT_LIT,
    },
    rangecoder::{LzmaRangeDecoder, Probability},
};

use super::{
    ALIGN_SIZE, DIST_MODEL_END, DIST_SLOTS, DIST_STATES, FULL_DISTANCES, LEN_HIGH_SYMBOLS,
    LEN_LOW_SYMBOLS, LEN_MID_SYMBOLS, LITERAL_CODERS_MAX, LITERAL_CODER_SIZE, POS_STATES_MAX,
    STATES,
};

// 定义状态转换表
static NEXT_STATE: [u32; 12] = [
    STATE_LIT_LIT,
    STATE_LIT_LIT,
    STATE_LIT_LIT,
    STATE_LIT_LIT,
    STATE_MATCH_LIT_LIT,
    STATE_REP_LIT_LIT,
    STATE_SHORTREP_LIT_LIT,
    STATE_MATCH_LIT,
    STATE_REP_LIT,
    STATE_SHORTREP_LIT,
    STATE_MATCH_LIT,
    STATE_REP_LIT,
];

#[derive(Debug)]
pub struct LzmaLengthDecoder {
    pub choice: Probability,
    pub choice2: Probability,
    pub low: [[Probability; LEN_LOW_SYMBOLS]; POS_STATES_MAX],
    pub mid: [[Probability; LEN_MID_SYMBOLS]; POS_STATES_MAX],
    pub high: [Probability; LEN_HIGH_SYMBOLS],
}
impl Default for LzmaLengthDecoder {
    fn default() -> Self {
        LzmaLengthDecoder {
            choice: Probability::default(),  // 假设 Probability 实现了 Default
            choice2: Probability::default(), // 假设 Probability 实现了 Default
            low: [[Probability::default(); LEN_LOW_SYMBOLS]; POS_STATES_MAX], // 初始化为默认值
            mid: [[Probability::default(); LEN_MID_SYMBOLS]; POS_STATES_MAX], // 初始化为默认值
            high: [Probability::default(); LEN_HIGH_SYMBOLS], // 初始化为默认值
        }
    }
}

#[derive(Debug)]
pub struct LzmaLzma1Decoder {
    // Probabilities
    pub literal: [[Probability; LITERAL_CODER_SIZE]; LITERAL_CODERS_MAX],
    pub is_match: [[Probability; POS_STATES_MAX]; STATES],
    pub is_rep: [Probability; STATES],
    pub is_rep0: [Probability; STATES],
    pub is_rep1: [Probability; STATES],
    pub is_rep2: [Probability; STATES],
    pub is_rep0_long: [[Probability; POS_STATES_MAX]; STATES],
    pub dist_slot: [[Probability; DIST_SLOTS]; DIST_STATES],
    pub pos_special: [Probability; FULL_DISTANCES - DIST_MODEL_END],
    pub pos_align: [Probability; ALIGN_SIZE],
    pub match_len_decoder: LzmaLengthDecoder,
    pub rep_len_decoder: LzmaLengthDecoder,

    // Decoder state
    pub rc: LzmaRangeDecoder,
    pub state: u32,
    pub rep0: u32,
    pub rep1: u32,
    pub rep2: u32,
    pub rep3: u32,
    pub pos_mask: u32,
    pub literal_context_bits: u32,
    pub literal_pos_mask: u32,
    pub uncompressed_size: LzmaVli,
    pub allow_eopm: bool,

    // State of incomplete symbol
    pub sequence: Sequence,
    pub probs: Box<[Probability]>, // 使用Box<[Probability]>，拥有数据所有权
    pub symbol: u32,
    pub limit: u32,
    pub offset: u32,
    pub len: u32,
}

impl Default for LzmaLzma1Decoder {
    fn default() -> Self {
        LzmaLzma1Decoder {
            // 初始化概率数组为默认值（假设 Probability 实现了 Default）
            literal: [[Probability::default(); LITERAL_CODER_SIZE]; LITERAL_CODERS_MAX],
            is_match: [[Probability::default(); POS_STATES_MAX]; STATES],
            is_rep: [Probability::default(); STATES],
            is_rep0: [Probability::default(); STATES],
            is_rep1: [Probability::default(); STATES],
            is_rep2: [Probability::default(); STATES],
            is_rep0_long: [[Probability::default(); POS_STATES_MAX]; STATES],
            dist_slot: [[Probability::default(); DIST_SLOTS]; DIST_STATES],
            pos_special: [Probability::default(); FULL_DISTANCES - DIST_MODEL_END],
            pos_align: [Probability::default(); ALIGN_SIZE],
            match_len_decoder: LzmaLengthDecoder::default(), // 假设 LzmaLengthDecoder 实现了 Default
            rep_len_decoder: LzmaLengthDecoder::default(), // 假设 LzmaLengthDecoder 实现了 Default

            // 初始化 Decoder state 为默认值
            rc: LzmaRangeDecoder::default(), // 假设 LzmaRangeDecoder 实现了 Default
            state: 0,                        // 假设 LzmaLzmaState 实现了 Default
            rep0: 0,
            rep1: 0,
            rep2: 0,
            rep3: 0,
            pos_mask: 0,
            literal_context_bits: 0,
            literal_pos_mask: 0,
            uncompressed_size: 0, // 使用合适的默认值类型
            allow_eopm: false,

            // 初始化不完整符号的状态
            sequence: Sequence::default(), // 假设 Sequence 实现了 Default
            probs: Box::new([Probability::default(); LITERAL_CODER_SIZE * LITERAL_CODERS_MAX]), // 使用Box::new，拥有数据所有权
            symbol: 0,
            limit: 0,
            offset: 0,
            len: 0,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Default, Debug)]
pub enum Sequence {
    #[default]
    Normalize,
    IsMatch,
    Literal,
    Literal1,
    Literal2,
    Literal3,
    Literal4,
    Literal5,
    Literal6,
    Literal7,
    LiteralMatched,
    LiteralMatched1,
    LiteralMatched2,
    LiteralMatched3,
    LiteralMatched4,
    LiteralMatched5,
    LiteralMatched6,
    LiteralMatched7,
    LiteralWrite,
    IsRep,
    MatchLenChoice,
    MatchLenLow0,
    MatchLenLow1,
    MatchLenLow2,
    MatchLenChoice2,
    MatchLenBitTree,
    MatchLenMid0,
    MatchLenMid1,
    MatchLenMid2,
    MatchLenHigh0,
    MatchLenHigh1,
    MatchLenHigh2,
    MatchLenHigh3,
    MatchLenHigh4,
    MatchLenHigh5,
    MatchLenHigh6,
    MatchLenHigh7,
    DistSlot,
    DistSlot1,
    DistSlot2,
    DistSlot3,
    DistSlot4,
    DistSlot5,
    DistModel,
    Direct,
    Align,
    Align0,
    Align1,
    Align2,
    Align3,
    Eopm,
    IsRep0,
    ShortRep,
    IsRep0Long,
    IsRep1,
    IsRep2,
    RepLenChoice,
    RepLenBitTree,
    RepLenChoice2,
    RepLenLow0,
    RepLenLow1,
    RepLenLow2,
    RepLenMid0,
    RepLenMid1,
    RepLenMid2,
    RepLenHigh0,
    RepLenHigh1,
    RepLenHigh2,
    RepLenHigh3,
    RepLenHigh4,
    RepLenHigh5,
    RepLenHigh6,
    RepLenHigh7,
    Copy,
}
