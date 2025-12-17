/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::{
    api::{
        LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaVli, LZMA_FILTER_LZMA1EXT, LZMA_LCLP_MAX,
        LZMA_LZMA1EXT_ALLOW_EOPM, LZMA_VLI_UNKNOWN,
    },
    bit_reset, bittree_reset,
    common::{LzmaFilterInfo, LzmaNextCoder},
    lz::{
        dict_get, dict_put, dict_repeat, lzma_lz_decoder_init, lzma_lz_decoder_memusage,
        LzCoderType, LzmaDict, LzmaLzDecoder, LzmaLzDecoderOptions,
    },
    lzma::{
        update_match, update_short_rep, ALIGN_BITS, DIST_MODEL_START, DIST_SLOT_BITS,
        LEN_HIGH_BITS, LEN_LOW_BITS, LEN_MID_BITS, LIT_STATES, MATCH_LEN_MIN, STATE_LIT_LIT,
        STATE_LIT_MATCH, STATE_MATCH_LIT, STATE_MATCH_LIT_LIT, STATE_NONLIT_MATCH, STATE_REP_LIT,
        STATE_REP_LIT_LIT, STATE_SHORTREP_LIT, STATE_SHORTREP_LIT_LIT,
    },
    rangecoder::{
        rc_read_init, LzmaRangeDecoder, Probability, RC_BIT_MODEL_TOTAL, RC_BIT_MODEL_TOTAL_BITS,
        RC_MOVE_BITS, RC_SHIFT_BITS, RC_TOP_VALUE,
    },
    rc_direct, rc_is_finished, rc_normalize, rc_reset,
};
use common::read32le;

use crate::{lzma::update_long_rep, rc_bit, rc_if_0, rc_update_0, rc_update_1};

use super::{
    is_lclppb_valid, ALIGN_SIZE, DIST_MODEL_END, DIST_SLOTS, DIST_STATES, FULL_DISTANCES,
    LEN_HIGH_SYMBOLS, LEN_LOW_SYMBOLS, LEN_MID_SYMBOLS, LITERAL_CODERS_MAX, LITERAL_CODER_SIZE,
    POS_STATES_MAX, STATES,
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
    pub probs: AtomicPtr<Probability>, // 使用Box<[Probability]>，拥有数据所有权
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
            probs: AtomicPtr::new(ptr::null_mut()), // 使用Box::new，拥有数据所有权
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

pub fn lzma_decode(
    coder_ptr: &mut LzCoderType,
    dictptr: &mut LzmaDict,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    let coder = match coder_ptr {
        LzCoderType::LzmaDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError,
    };

    // 初始化 Range Decoder
    let ret = rc_read_init(&mut coder.rc, input, in_pos, in_size);
    if ret != LzmaRet::StreamEnd {
        return ret;
    }

    // 复制字典
    let mut dict = dictptr.clone();
    let dict_start = dict.pos;

    //rc_to_local(&mut coder.rc, *in_pos);
    let mut rc = coder.rc.clone();
    let mut rc_in_pos = *in_pos;
    let mut rc_bound: u32 = 0;

    // 初始化变量
    let mut state = coder.state;
    let mut rep0 = coder.rep0;
    let mut rep1 = coder.rep1;
    let mut rep2 = coder.rep2;
    let mut rep3 = coder.rep3;
    let pos_mask = coder.pos_mask;

    let mut probs_line_ref = coder.probs.load(Ordering::SeqCst); // 初始化coder.probs 的数据
    let mut probs_data_offset: isize = 0;

    let mut symbol: u32 = coder.symbol;
    let mut limit = coder.limit;

    let mut offset = coder.offset;
    let mut len = coder.len;
    let literal_pos_mask = coder.literal_pos_mask;
    let literal_context_bits = coder.literal_context_bits;
    let mut pos_state = dict.pos & pos_mask as usize;
    let mut ret = LzmaRet::Ok;
    let mut match_bit: u32 = 0;
    let mut subcoder_index: u32 = 0;
    let mut byte: u8 = 0;

    let mut goto_flag = false; // 是否跳转到out处

    // 检查是否允许 EOPM
    let mut eopm_is_valid = coder.uncompressed_size == u64::MAX;
    let mut might_finish_without_eopm = false;

    if coder.uncompressed_size != u64::MAX
        && (coder.uncompressed_size as usize) <= dict.limit - dict.pos
    {
        dict.limit = dict.pos + coder.uncompressed_size as usize;
        might_finish_without_eopm = true;
    }

    let mut next_sequence = coder.sequence;

    let mut udate_pos_state = false; // 第一次循环不用进入判断条件
    unsafe {
        loop {
            if udate_pos_state {
                pos_state = dict.pos & pos_mask as usize;
                udate_pos_state = false;
            }

            match next_sequence {
                // 核心分支1
                Sequence::Normalize | Sequence::IsMatch => {
                    if might_finish_without_eopm && dict.pos == dict.limit {
                        rc_normalize!(
                            rc,
                            coder.sequence = Sequence::Normalize,
                            rc_in_pos,
                            in_size,
                            input
                        );
                        if rc.code == 0 {
                            ret = LzmaRet::StreamEnd;
                            break;
                        }
                        if !coder.allow_eopm {
                            ret = LzmaRet::DataError;
                            break;
                        }
                        eopm_is_valid = true;
                    }

                    if rc_if_0!(
                        rc,
                        coder.is_match[state as usize][pos_state as usize],
                        coder.sequence = Sequence::IsMatch,
                        rc_in_pos,
                        in_size,
                        input,
                        rc_bound,
                        goto_flag
                    ) {
                        //println!("is_match[{}][{}]: {}, rc_bound: {}, rc.code: {}, rc.range: {}, rc_in_pos: {}, in_size: {}, input[rc_in_pos]: {}", state, pos_state, coder.is_match[state as usize][pos_state as usize], rc_bound, rc.code, rc.range, rc_in_pos,in_size,input[rc_in_pos]);
                        rc_update_0!(
                            rc,
                            coder.is_match[state as usize][pos_state as usize],
                            rc_bound
                        );

                        //println!("is_match[{}][{}]: {}, rc_bound: {}, rc.code: {}, rc.range: {}, rc_in_pos: {}, in_size: {}, input[rc_in_pos]: {}", state, pos_state, coder.is_match[state as usize][pos_state as usize], rc_bound, rc.code, rc.range, rc_in_pos,in_size,input[rc_in_pos]);

                        // 字面量解析
                        let dg = dict_get(&dict, 0);

                        let index = ((dict.pos & literal_pos_mask as usize)
                            << literal_context_bits as usize)
                            + ((dg as u32 >> (8 - literal_context_bits)) as usize);
                        probs_line_ref = coder.literal[index].as_mut_ptr();

                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        symbol = 1;

                        if state < LIT_STATES.try_into().unwrap() {
                            next_sequence = Sequence::Literal;
                        } else {
                            len = (dict_get(&dict, rep0) as u32) << 1;
                            offset = 0x100;
                            next_sequence = Sequence::LiteralMatched;
                        }
                    } else {
                        //从C语言的代码可以看出，这个分支在 rc_if_0 为真的时候，是执行不到的，因为在LiteralWrite 中有个continue,所以这段代码放到 else 中，和C 语言的稍有区别，C 语言没有这个else
                        rc_update_1!(
                            rc,
                            coder.is_match[state as usize][pos_state as usize],
                            rc_bound
                        );

                        next_sequence = Sequence::IsRep;
                    }
                }
                // 承接 Is match 分支
                Sequence::Literal => {
                    // 解码字面量（无匹配字节）
                    while symbol < (1 << 8) {
                        // rc_bit!(
                        //     rc,
                        //     probs_line_ref[(symbol as isize+ probs_data_offset) as usize],
                        //     symbol,
                        //     (),
                        //     (),
                        //     rc_in_pos,
                        //     in_size,
                        //     input,
                        //     rc_bound,
                        //     coder.sequence = Sequence::Literal,
                        //     goto_flag
                        // );
                        if rc.range < (1 << 24) {
                            if rc_in_pos == in_size {
                                goto_flag = true;
                                coder.sequence = Sequence::Literal;
                                break;
                            }
                            rc.range <<= 8;
                            rc.code = (rc.code << 8) | (input[rc_in_pos] as u32);
                            rc_in_pos += 1;
                        }
                        rc_bound = (rc.range >> 11).wrapping_mul(
                            *probs_line_ref.offset((symbol as isize + probs_data_offset) as isize)
                                as u32,
                        );
                        if rc.code < rc_bound {
                            rc.range = rc_bound;

                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur + (((1 << 11) - cur) >> 5));
                            p.write(new_val);
                            symbol = (symbol << 1);
                        } else {
                            rc.range = rc.range.wrapping_sub(rc_bound);
                            rc.code = rc.code.wrapping_sub(rc_bound);

                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur - ((cur) >> 5));
                            p.write(new_val);

                            symbol = (symbol << 1) + 1;
                        }
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    state = NEXT_STATE[state as usize];

                    next_sequence = Sequence::LiteralWrite;
                }
                // 承接 Is match 分支
                Sequence::LiteralMatched => {
                    // 解码字面量（有匹配字节）
                    while symbol < (1 << 8) {
                        let match_bit = len & offset;
                        let subcoder_index = offset + match_bit + symbol;
                        // rc_bit!(
                        //     rc,
                        //     probs_line_ref[(subcoder_index as isize + probs_data_offset) as usize],
                        //     symbol,
                        //     offset = offset & !match_bit,
                        //     offset = offset & match_bit,
                        //     rc_in_pos,
                        //     in_size,
                        //     input,
                        //     rc_bound,
                        //     coder.sequence = Sequence::LiteralMatched,
                        //     goto_flag
                        // );

                        if rc.range < (1 << 24) {
                            if rc_in_pos == in_size {
                                goto_flag = true;
                                coder.sequence = Sequence::LiteralMatched;
                                break;
                            }
                            rc.range <<= 8;
                            rc.code = (rc.code << 8) | (input[rc_in_pos] as u32);
                            rc_in_pos += 1;
                        }
                        rc_bound = (rc.range >> 11).wrapping_mul(
                            *probs_line_ref.offset(subcoder_index as isize + probs_data_offset)
                                as u32,
                        );
                        if rc.code < rc_bound {
                            rc.range = rc_bound;

                            let p =
                                probs_line_ref.offset(subcoder_index as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur + (((1 << 11) - cur) >> 5));
                            p.write(new_val);
                            symbol = (symbol << 1);
                            offset = offset & !match_bit;
                        } else {
                            rc.range = rc.range.wrapping_sub(rc_bound);
                            rc.code = rc.code.wrapping_sub(rc_bound);

                            let p =
                                probs_line_ref.offset(subcoder_index as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur - ((cur) >> 5));
                            p.write(new_val);

                            symbol = (symbol << 1) + 1;
                            offset = offset & match_bit;
                        }

                        len <<= 1;
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    state = NEXT_STATE[state as usize];
                    next_sequence = Sequence::LiteralWrite;
                }

                Sequence::LiteralWrite => {
                    if dict_put(&mut dict, symbol as u8) {
                        coder.sequence = Sequence::LiteralWrite;
                        break;
                    }
                    next_sequence = Sequence::IsMatch;
                    udate_pos_state = true;
                    continue;
                }
                //// 核心分支2  这里是一个阶段， 直到 Sequence::IsRep 开始
                Sequence::IsRep => {
                    if rc_if_0!(
                        rc,
                        coder.is_rep[state as usize],
                        next_sequence = Sequence::IsRep,
                        rc_in_pos,
                        in_size,
                        input,
                        rc_bound,
                        goto_flag
                    ) {
                        // 不是重复匹配
                        rc_update_0!(rc, coder.is_rep[state as usize], rc_bound);

                        let mut new_state = state;
                        update_match(&mut new_state);
                        state = new_state;

                        // 保存最近的三个匹配距离，以防有重复匹配
                        rep3 = rep2;
                        rep2 = rep1;
                        rep1 = rep0;

                        next_sequence = Sequence::MatchLenChoice;
                    } else {
                        rc_update_1!(rc, coder.is_rep[state as usize], rc_bound);
                        if !dict_is_distance_valid(&dict, 0) {
                            ret = LzmaRet::DataError;
                            break;
                        }

                        next_sequence = Sequence::IsRep0;
                    }
                }

                // 承接 IsRep 分支 rc_if_0 分支
                Sequence::MatchLenChoice => {
                    if rc.range < (1 << 24) {
                        if rc_in_pos == in_size {
                            coder.sequence = Sequence::MatchLenChoice;
                            break;
                        }
                        rc.range <<= 8;
                        rc.code = (rc.code << 8) | u32::from(input[rc_in_pos]);
                        rc_in_pos += 1;
                    }
                    rc_bound = (rc.range >> 11) * u32::from(coder.match_len_decoder.choice);

                    // 根据概率解码
                    if rc.code < rc_bound {
                        rc.range = rc_bound;
                        coder.match_len_decoder.choice +=
                            ((1 << 11) - coder.match_len_decoder.choice) >> 5;
                        probs_line_ref =
                            coder.match_len_decoder.low[pos_state as usize].as_mut_ptr();
                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        limit = (1 << 3);
                        len = 2;
                        symbol = 1;
                        next_sequence = Sequence::MatchLenBitTree;
                    } else {
                        rc.range -= rc_bound;
                        rc.code -= rc_bound;
                        coder.match_len_decoder.choice -= coder.match_len_decoder.choice >> 5;
                        next_sequence = Sequence::MatchLenChoice2;
                    }
                }
                Sequence::MatchLenChoice2 => {
                    if rc.range < (1 << 24) {
                        if rc_in_pos == in_size {
                            coder.sequence = Sequence::MatchLenChoice2;
                            break;
                        }
                        rc.range <<= 8;
                        rc.code = (rc.code << 8) | u32::from(input[rc_in_pos]);
                        rc_in_pos += 1;
                    }
                    rc_bound = (rc.range >> 11) * u32::from(coder.match_len_decoder.choice2);
                    if rc.code < rc_bound {
                        rc.range = rc_bound;
                        coder.match_len_decoder.choice2 +=
                            ((1 << 11) - coder.match_len_decoder.choice2) >> 5;
                        probs_line_ref =
                            coder.match_len_decoder.mid[pos_state as usize].as_mut_ptr();
                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        limit = (1 << 3);
                        len = 2 + (1 << 3);
                    } else {
                        rc.range -= rc_bound;
                        rc.code -= rc_bound;
                        coder.match_len_decoder.choice2 -= (coder.match_len_decoder.choice2) >> 5;

                        probs_line_ref = coder.match_len_decoder.high.as_mut_ptr();
                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        limit = (1 << 8);
                        len = 2 + (1 << 3) + (1 << 3);
                    }
                    symbol = 1;
                    next_sequence = Sequence::MatchLenBitTree; // 走向了 slot
                }
                Sequence::MatchLenBitTree => {
                    while symbol < limit {
                        if rc.range < (1 << 24) {
                            if rc_in_pos == in_size {
                                goto_flag = true;
                                coder.sequence = Sequence::MatchLenBitTree;
                                break;
                            }
                            rc.range <<= 8;
                            rc.code = (rc.code << 8) | u32::from(input[rc_in_pos]);
                            rc_in_pos += 1;
                        }

                        rc_bound = (rc.range >> 11)
                            * u32::from(
                                *probs_line_ref
                                    .offset((symbol as isize + probs_data_offset) as isize),
                            );
                        if rc.code < rc_bound {
                            rc.range = rc_bound;
                            // probs_line_ref[(symbol as isize + probs_data_offset) as usize] += ((1 << 11) - probs_line_ref[(symbol as isize + probs_data_offset) as usize]) >> 5;
                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur + (((1 << 11) - cur) >> 5));
                            p.write(new_val);
                            symbol <<= 1;
                        } else {
                            rc.range -= rc_bound;
                            rc.code -= rc_bound;
                            // probs_line_ref[(symbol as isize + probs_data_offset) as usize] -= probs_line_ref[(symbol as isize + probs_data_offset) as usize] >> 5;
                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur - ((cur) >> 5));
                            p.write(new_val);
                            symbol = (symbol << 1) + 1;
                        }
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    len += symbol - limit;
                    let tmp = get_dist_state(len);
                    probs_line_ref = coder.dist_slot[get_dist_state(len) as usize].as_mut_ptr();
                    probs_data_offset = 0; // 重置 probs_data_offset 为 0
                    symbol = 1;
                    next_sequence = Sequence::DistSlot;
                }
                Sequence::DistSlot => {
                    while symbol < DIST_SLOTS as u32 {
                        // rc_bit!(
                        //     rc,
                        //     probs_line_ref[(symbol as isize + probs_data_offset) as usize],
                        //     symbol,
                        //     (),
                        //     (),
                        //     rc_in_pos,
                        //     in_size,
                        //     input,
                        //     rc_bound,
                        //     coder.sequence = Sequence::DistSlot,
                        //     goto_flag
                        // );

                        if rc.range < crate::rangecoder::range_common::RC_TOP_VALUE {
                            if rc_in_pos == in_size {
                                goto_flag = true;
                                coder.sequence = Sequence::DistSlot;
                                break;
                            }
                            rc.range <<= crate::rangecoder::range_common::RC_SHIFT_BITS;
                            rc.code = (rc.code << crate::rangecoder::range_common::RC_SHIFT_BITS)
                                | (input[rc_in_pos] as u32);
                            rc_in_pos += 1;
                        }
                        rc_bound = (rc.range >> 11).wrapping_mul(
                            *probs_line_ref.offset((symbol as isize + probs_data_offset) as isize)
                                as u32,
                        );
                        if rc.code < rc_bound {
                            rc.range = rc_bound;

                            // probs_line_ref[(symbol as isize + probs_data_offset) as usize] = ((probs_line_ref[(symbol as isize + probs_data_offset) as usize] as u32).wrapping_add(((crate::rangecoder::range_common::RC_BIT_MODEL_TOTAL - probs_line_ref[(symbol as isize + probs_data_offset) as usize] as u32) >> crate::rangecoder::range_common::RC_MOVE_BITS))) as u16;
                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur + (((1 << 11) - cur) >> 5));
                            p.write(new_val);
                            symbol = (symbol << 1);
                        } else {
                            rc.range = rc.range.wrapping_sub(rc_bound);
                            rc.code = rc.code.wrapping_sub(rc_bound);

                            // let tmp = (probs_line_ref[(symbol as isize + probs_data_offset) as usize] as u32 - (probs_line_ref[(symbol as isize + probs_data_offset) as usize] as u32 >> crate::rangecoder::range_common::RC_MOVE_BITS)) as u16;
                            // // probs_line_ref[(symbol as isize + probs_data_offset) as usize] = (probs_line_ref[(symbol as isize + probs_data_offset) as usize] as u32 - (probs_line_ref[(symbol as isize + probs_data_offset) as usize] as u32 >> crate::rangecoder::range_common::RC_MOVE_BITS)) as u16;

                            // probs_line_ref[(symbol as isize + probs_data_offset) as usize] = tmp;
                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur - ((cur) >> 5));
                            p.write(new_val);

                            symbol = (symbol << 1) + 1;
                        }
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    symbol -= DIST_SLOTS as u32;
                    // println!("symbol = {}", symbol);

                    if symbol < DIST_MODEL_START as u32 {
                        // Match distances [0, 3] have only two bits.
                        rep0 = symbol;

                        if !dict_is_distance_valid(&dict, rep0 as usize) {
                            ret = LzmaRet::DataError;
                            break;
                        }
                        next_sequence = Sequence::Copy;
                    } else {
                        // Decode the lowest [1, 29] bits of
                        // the match distance.
                        limit = (symbol >> 1) - 1;
                        // println!("limit = {}", limit);
                        assert!(limit >= 1 && limit <= 30);

                        rep0 = 2 + (symbol & 1);

                        if (symbol < DIST_MODEL_END.try_into().unwrap()) {
                            // Prepare to decode the low bits for
                            // a distance of [4, 127].
                            assert!(limit <= 5);
                            rep0 <<= limit;
                            assert!(rep0 <= 96);

                            // -1 is fine, because we start
                            // decoding at probs[1], not probs[0].
                            // NOTE: This violates the C standard,
                            // since we are doing pointer
                            // arithmetic past the beginning of
                            // the array.
                            // 等价于C代码中的: assert((int32_t)(rep0 - symbol - 1) >= -1);
                            // 在C中，当rep0 < symbol + 1时，无符号减法会溢出，转换为int32_t后为负数
                            // 但在这种情况下，我们仍然允许-1，因为注释说明"从probs[1]开始解码，而不是probs[0]"
                            assert!((rep0 as i32 - symbol as i32 - 1) >= -1);

                            let base: isize = (rep0 as isize) - (symbol as isize) - 1;
                            assert!(base <= 82);
                            assert!(base >= -1 && base <= (coder.pos_special.len() as isize) - 1); // 对应 C 的断言
                                                                                                   // 只允许 base >= 0 时用切片，否则 panic 或特殊处理

                            probs_line_ref = if base >= 0 {
                                probs_data_offset = 0;

                                coder.pos_special[(base) as usize..].as_mut_ptr()
                            //这里还是使用实际的位置，通过 probs_data_offset 和C代码保持一致，抵消总是从 probs[1] 计算的影响
                            } else {
                                // base == -1 时，probs[1] 恰好是 pos_special[0]
                                // 这里可以用偏移量修正
                                probs_data_offset = -1;
                                coder.pos_special[0..].as_mut_ptr()
                            };

                            //  probs_line_ref = &mut coder.pos_special[(rep0 - symbol - 1) as usize..];
                            symbol = 1;
                            offset = 0;
                            next_sequence = Sequence::DistModel;
                        } else {
                            assert!(symbol >= 14);
                            assert!(limit >= 6);
                            limit -= ALIGN_BITS as u32;

                            assert!(limit >= 2);

                            next_sequence = Sequence::Direct;
                        }
                    }
                }
                Sequence::DistModel => {
                    while offset < limit as u32 {
                        // rc_bit!(
                        //     rc,
                        //     probs_line_ref[(symbol as isize + probs_data_offset) as usize],
                        //     symbol,
                        //     (),
                        //     rep0 = rep0 + (1 << offset),
                        //     rc_in_pos,
                        //     in_size,
                        //     input,
                        //     rc_bound,
                        //     coder.sequence = Sequence::DistModel,
                        //     goto_flag
                        // );

                        if rc.range < (1 << 24) {
                            if rc_in_pos == in_size {
                                goto_flag = true;
                                coder.sequence = Sequence::DistModel;
                                break;
                            }
                            rc.range <<= 8;
                            rc.code = (rc.code << 8) | (input[rc_in_pos] as u32);
                            rc_in_pos += 1;
                        }
                        rc_bound = (rc.range >> 11).wrapping_mul(
                            *probs_line_ref.offset((symbol as isize + probs_data_offset) as isize)
                                as u32,
                        );
                        if rc.code < rc_bound {
                            rc.range = rc_bound;

                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur + (((1 << 11) - cur) >> 5));
                            p.write(new_val);
                            symbol = (symbol << 1);
                        } else {
                            rc.range = rc.range.wrapping_sub(rc_bound);
                            rc.code = rc.code.wrapping_sub(rc_bound);

                            let p = probs_line_ref.offset(symbol as isize + probs_data_offset);
                            let cur = p.read();
                            let new_val = (cur - ((cur) >> 5));
                            p.write(new_val);

                            symbol = (symbol << 1) + 1;
                            rep0 = rep0 + (1 << offset);
                        }

                        offset += 1;
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    if !dict_is_distance_valid(&dict, rep0 as usize) {
                        ret = LzmaRet::DataError;
                        break;
                    }
                    next_sequence = Sequence::Copy;
                }
                Sequence::Direct => {
                    while limit > 0 {
                        rc_direct!(
                            rc,
                            rep0,
                            coder.sequence = Sequence::Direct,
                            rc_in_pos,
                            in_size,
                            input,
                            rc_bound,
                            goto_flag
                        );
                        limit -= 1;
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    rep0 <<= ALIGN_BITS;
                    symbol = 1;
                    offset = 0;

                    next_sequence = Sequence::Align;
                }
                Sequence::Align => {
                    while offset < ALIGN_BITS as u32 {
                        rc_bit!(
                            rc,
                            coder.pos_align[symbol as usize],
                            symbol,
                            (),
                            rep0 = rep0 + (1 << offset),
                            rc_in_pos,
                            in_size,
                            input,
                            rc_bound,
                            coder.sequence = Sequence::Align,
                            goto_flag
                        );
                        offset += 1;
                    }
                    if goto_flag {
                        goto_flag = false;
                        break;
                    }
                    if rep0 == u32::MAX {
                        if !eopm_is_valid {
                            ret = LzmaRet::DataError;
                            break;
                        }
                        next_sequence = Sequence::Eopm;
                    } else {
                        if !dict_is_distance_valid(&dict, rep0 as usize) {
                            ret = LzmaRet::DataError;
                            break;
                        }
                        next_sequence = Sequence::Copy;
                    }
                }
                Sequence::Eopm => {
                    //  IsRep中的if 执行完，执行的这里，循环肯定会退出
                    rc_normalize!(
                        rc,
                        coder.sequence = Sequence::Eopm,
                        rc_in_pos,
                        in_size,
                        input
                    );
                    ret = if rc_is_finished!(rc) {
                        LzmaRet::StreamEnd
                    } else {
                        LzmaRet::DataError
                    };
                    break;
                }

                // 承接 IsRep rc_if_0  的 else 分支
                Sequence::IsRep0 => {
                    if rc_if_0!(
                        rc,
                        coder.is_rep0[state as usize],
                        coder.sequence = Sequence::IsRep0,
                        rc_in_pos,
                        in_size,
                        input,
                        rc_bound,
                        goto_flag
                    ) {
                        rc_update_0!(rc, coder.is_rep0[state as usize], rc_bound);
                        next_sequence = Sequence::IsRep0Long;
                    } else {
                        rc_update_1!(rc, coder.is_rep0[state as usize], rc_bound);
                        next_sequence = Sequence::IsRep1;
                    }
                }
                // SEQ_IS_REP0 的 tc_if_0 true分支
                Sequence::IsRep0Long => {
                    if rc_if_0!(
                        rc,
                        coder.is_rep0_long[state as usize][pos_state as usize],
                        coder.sequence = Sequence::IsRep0Long,
                        rc_in_pos,
                        in_size,
                        input,
                        rc_bound,
                        goto_flag
                    ) {
                        rc_update_0!(
                            rc,
                            coder.is_rep0_long[state as usize][pos_state as usize],
                            rc_bound
                        );

                        update_short_rep(&mut state);

                        next_sequence = Sequence::ShortRep; // 注意 这个分支执行后，总会continue
                    } else {
                        rc_update_1!(
                            rc,
                            coder.is_rep0_long[state as usize][pos_state as usize],
                            rc_bound
                        );

                        // 这个和C 语言有区别，应该放到这里，因为 if 的true 分支，总是有continue。
                        update_long_rep(&mut state);

                        // Decode the length of the repeated match.
                        next_sequence = Sequence::RepLenChoice;
                    }
                }
                Sequence::ShortRep => {
                    let byte = dict_get(&dict, rep0);
                    if dict_put(&mut dict, byte) {
                        break;
                    }
                    next_sequence = Sequence::IsMatch;
                    udate_pos_state = true;
                    continue;
                }

                Sequence::IsRep1 => {
                    if rc_if_0!(
                        rc,
                        coder.is_rep1[state as usize],
                        coder.sequence = Sequence::IsRep1,
                        rc_in_pos,
                        in_size,
                        input,
                        rc_bound,
                        goto_flag
                    ) {
                        rc_update_0!(rc, coder.is_rep1[state as usize], rc_bound);

                        let distance = rep1;
                        rep1 = rep0;
                        rep0 = distance;

                        update_long_rep(&mut state);

                        // Decode the length of the repeated match.
                        next_sequence = Sequence::RepLenChoice;
                    } else {
                        rc_update_1!(rc, coder.is_rep1[state as usize], rc_bound);
                        next_sequence = Sequence::IsRep2;
                    }
                }

                Sequence::IsRep2 => {
                    if rc_if_0!(
                        rc,
                        coder.is_rep2[state as usize],
                        coder.sequence = Sequence::IsRep2,
                        rc_in_pos,
                        in_size,
                        input,
                        rc_bound,
                        goto_flag
                    ) {
                        rc_update_0!(rc, coder.is_rep2[state as usize], rc_bound);
                        let distance = rep2;
                        rep2 = rep1;
                        rep1 = rep0;
                        rep0 = distance;
                    } else {
                        rc_update_1!(rc, coder.is_rep2[state as usize], rc_bound);
                        let distance = rep3;
                        rep3 = rep2;
                        rep2 = rep1;
                        rep1 = rep0;
                        rep0 = distance;
                    }
                    update_long_rep(&mut state);

                    // Decode the length of the repeated match.
                    // len_decode(len, coder.rep_len_decoder, pos_state, SEQ_REP_LEN);
                    next_sequence = Sequence::RepLenChoice;
                }
                Sequence::RepLenChoice => {
                    if rc.range < (1 << 24) {
                        if rc_in_pos == in_size {
                            coder.sequence = Sequence::RepLenChoice;
                            break;
                        }
                        rc.range <<= 8;
                        rc.code = (rc.code << 8) | u32::from(input[rc_in_pos]);
                        rc_in_pos += 1;
                    }
                    rc_bound = (rc.range >> 11) * u32::from(coder.rep_len_decoder.choice);

                    // 根据概率解码
                    if rc.code < rc_bound {
                        rc.range = rc_bound;
                        coder.rep_len_decoder.choice +=
                            ((1 << 11) - coder.rep_len_decoder.choice) >> 5;
                        probs_line_ref = coder.rep_len_decoder.low[pos_state as usize].as_mut_ptr();
                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        limit = (1 << 3);
                        len = 2;
                        symbol = 1;
                        next_sequence = Sequence::RepLenBitTree;
                    } else {
                        rc.range -= rc_bound;
                        rc.code -= rc_bound;
                        coder.rep_len_decoder.choice -= coder.rep_len_decoder.choice >> 5;
                        next_sequence = Sequence::RepLenChoice2;
                    }
                }
                Sequence::RepLenChoice2 => {
                    if rc.range < (1 << 24) {
                        if rc_in_pos == in_size {
                            coder.sequence = Sequence::RepLenChoice2;
                            break;
                        }
                        rc.range <<= 8;
                        rc.code = (rc.code << 8) | u32::from(input[rc_in_pos]);
                        rc_in_pos += 1;
                    }
                    rc_bound = (rc.range >> 11) * u32::from(coder.rep_len_decoder.choice2);
                    if rc.code < rc_bound {
                        rc.range = rc_bound;
                        coder.rep_len_decoder.choice2 +=
                            ((1 << 11) - coder.rep_len_decoder.choice2) >> 5;
                        probs_line_ref = coder.rep_len_decoder.mid[pos_state as usize].as_mut_ptr();
                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        limit = (1 << 3);
                        len = 2 + (1 << 3);
                    } else {
                        rc.range -= rc_bound;
                        rc.code -= rc_bound;
                        coder.rep_len_decoder.choice2 -= (coder.rep_len_decoder.choice2) >> 5;

                        probs_line_ref = coder.rep_len_decoder.high.as_mut_ptr();
                        probs_data_offset = 0; // 重置 probs_data_offset 为 0
                        limit = (1 << 8);
                        len = 2 + (1 << 3) + (1 << 3);
                    }
                    symbol = 1;
                    next_sequence = Sequence::RepLenBitTree;
                }
                Sequence::RepLenBitTree => {
                    while symbol < limit {
                        if rc.range < (1 << 24) {
                            if rc_in_pos == in_size {
                                coder.sequence = Sequence::RepLenBitTree;
                                break;
                            }
                            rc.range <<= 8;
                            rc.code = (rc.code << 8) | u32::from(input[rc_in_pos]);
                            rc_in_pos += 1;
                        }

                        rc_bound =
                            (rc.range >> 11) * u32::from(*probs_line_ref.offset(symbol as isize));
                        if rc.code < rc_bound {
                            rc.range = rc_bound;

                            // probs_line_ref.offset(symbol as usize) += ((1 << 11) - probs_line_ref.offset(symbol as usize)) >> 5;

                            // Read current probability, compute updated value and write it back.
                            let p = probs_line_ref.offset(symbol as isize);
                            let cur = p.read() as u32;
                            let new_val = (cur + (((1u32 << 11) - cur) >> 5)) as u16;
                            p.write(new_val);

                            symbol <<= 1;
                        } else {
                            rc.range -= rc_bound;
                            rc.code -= rc_bound;
                            // probs_line_ref[symbol as usize] -= probs_line_ref[symbol as usize] >> 5;
                            let p = probs_line_ref.offset(symbol as isize);
                            let cur = p.read() as u32;
                            let new_val = (cur - (cur >> 5)) as u16;
                            p.write(new_val);
                            symbol = (symbol << 1) + 1;
                        }
                    }
                    len += symbol - limit;

                    next_sequence = Sequence::Copy;
                }

                // 核心分支3
                Sequence::Copy => {
                    let mut len_usize = len as usize;
                    if dict_repeat(&mut dict, rep0 as usize, &mut len_usize) {
                        len = len_usize as u32;
                        coder.sequence = Sequence::Copy;
                        break;
                    }
                    len = len_usize as u32;
                    next_sequence = Sequence::IsMatch;
                    udate_pos_state = true;
                }

                _ => break,
            }
        }

        // out:
        // 更新状态
        *dictptr = dict.clone();
        dictptr.pos = dict.pos;
        dictptr.full = dict.full;
        coder.rc = rc;
        *in_pos = rc_in_pos;
        coder.state = state;
        coder.rep0 = rep0;
        coder.rep1 = rep1;
        coder.rep2 = rep2;
        coder.rep3 = rep3;
        // coder.probs = probs_line_ref.to_vec().into_boxed_slice();
        coder.probs.store(probs_line_ref, Ordering::SeqCst);
        coder.symbol = symbol;
        coder.limit = limit;
        coder.offset = offset;
        coder.len = len;

        if coder.uncompressed_size != u64::MAX {
            coder.uncompressed_size -= (dict.pos - dict_start) as u64;
            if coder.uncompressed_size == 0
                && ret == LzmaRet::Ok
                && (coder.sequence == Sequence::LiteralWrite
                    || coder.sequence == Sequence::ShortRep
                    || coder.sequence == Sequence::Copy)
            {
                ret = LzmaRet::DataError;
            }
        }

        if ret == LzmaRet::StreamEnd {
            coder.rc.range = 0xFFFFFFFF;
            coder.rc.code = 0;
            coder.rc.init_bytes_left = 5;
            coder.sequence = Sequence::IsMatch;
        }
        ret
    }
}

#[inline]
fn literal_subcoder(
    probs: &[[u16; 0x300]],
    lc: u32,
    lp_mask: u32,
    pos: usize,
    prev_byte: u8,
) -> &[u16] {
    let index =
        ((pos & lp_mask as usize) << lc as usize) + ((prev_byte as u32 >> (8 - lc)) as usize);
    &probs[index]
}

#[inline]
fn literal_subcoder_mut(
    probs: &mut [[u16; 0x300]],
    lc: u32,
    lp_mask: u32,
    pos: usize,
    prev_byte: u8,
) -> &mut [u16] {
    let index =
        ((pos & lp_mask as usize) << lc as usize) + ((prev_byte as u32 >> (8 - lc)) as usize);
    &mut probs[index]
}

#[inline]
fn get_dist_state(len: u32) -> u32 {
    if len < (DIST_STATES + MATCH_LEN_MIN) as u32 {
        len - MATCH_LEN_MIN as u32
    } else {
        (DIST_STATES - 1) as u32
    }
}

#[inline]
pub fn dict_is_distance_valid(dict: &LzmaDict, distance: usize) -> bool {
    dict.full > distance
}

// fn dict_repeat(dict: &mut LzmaDict, rep0: u32, len: &mut u32) -> bool {
//     if *len == 0 {
//         return false;
//     }

//     let mut remaining = *len;
//     while remaining > 0 {
//         let byte = dict_get(dict, rep0);
//         if dict_put(dict, byte) {
//             *len = remaining;
//             return true;
//         }
//         remaining -= 1;
//     }
//     *len = 0;
//     false
// }

fn lzma_decoder_uncompressed(
    coder_ptr: &mut LzCoderType,
    uncompressed_size: LzmaVli,
    allow_eopm: bool,
) {
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        LzCoderType::LzmaDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    coder.uncompressed_size = uncompressed_size;
    coder.allow_eopm = allow_eopm;
}
fn literal_init(probs: &mut [[Probability; LITERAL_CODER_SIZE]], lc: u32, lp: u32) {
    assert!(lc + lp <= LZMA_LCLP_MAX);

    let coders = 1 << (lc + lp);

    for i in 0..coders {
        for j in 0..LITERAL_CODER_SIZE {
            bit_reset!(probs[i][j]);
        }
    }
}
fn lzma_decoder_reset(coder_ptr: &mut LzCoderType, opt: &LzmaOptionsType) {
    // let coder: &mut LzmaLzma1Decoder = coder_ptr;
    let coder = match coder_ptr {
        LzCoderType::LzmaDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    let options: &LzmaOptionsLzma = opt.as_lzma_options_lzma().unwrap();

    // 计算 pos_mask。我们不需要直接使用 pos_bits。
    coder.pos_mask = (1 << options.pb) - 1;

    // 初始化字面值解码器。
    literal_init(&mut coder.literal, options.lc, options.lp);

    coder.literal_context_bits = options.lc;
    coder.literal_pos_mask = (1 << options.lp) - 1;

    // 状态初始化
    coder.state = STATE_LIT_LIT;
    coder.rep0 = 0;
    coder.rep1 = 0;
    coder.rep2 = 0;
    coder.rep3 = 0;
    coder.pos_mask = (1 << options.pb) - 1;

    // 重置范围解码器
    rc_reset!(&mut coder.rc);

    // 重置比特和比特树解码器
    for i in 0..STATES {
        for j in 0..=coder.pos_mask as usize {
            bit_reset!(coder.is_match[i][j]);
            bit_reset!(coder.is_rep0_long[i][j]);
        }

        bit_reset!(coder.is_rep[i]);
        bit_reset!(coder.is_rep0[i]);
        bit_reset!(coder.is_rep1[i]);
        bit_reset!(coder.is_rep2[i]);
    }

    for i in 0..DIST_STATES {
        bittree_reset!(coder.dist_slot[i], DIST_SLOT_BITS);
    }

    for i in 0..FULL_DISTANCES - DIST_MODEL_END {
        bit_reset!(coder.pos_special[i]);
    }

    bittree_reset!(coder.pos_align, ALIGN_BITS);

    // 长度解码器（也包括比特/比特树）
    let num_pos_states = 1 << options.pb;
    bit_reset!(coder.match_len_decoder.choice);
    bit_reset!(coder.match_len_decoder.choice2);
    bit_reset!(coder.rep_len_decoder.choice);
    bit_reset!(coder.rep_len_decoder.choice2);

    for pos_state in 0..num_pos_states {
        bittree_reset!(coder.match_len_decoder.low[pos_state], LEN_LOW_BITS);
        bittree_reset!(coder.match_len_decoder.mid[pos_state], LEN_MID_BITS);

        bittree_reset!(coder.rep_len_decoder.low[pos_state], LEN_LOW_BITS);
        bittree_reset!(coder.rep_len_decoder.mid[pos_state], LEN_MID_BITS);
    }

    bittree_reset!(coder.match_len_decoder.high, LEN_HIGH_BITS);
    bittree_reset!(coder.rep_len_decoder.high, LEN_HIGH_BITS);

    coder.sequence = Sequence::IsMatch;
    coder.probs = AtomicPtr::new(ptr::null_mut()); // 使用Box::new，拥有数据所有权
    coder.symbol = 0;
    coder.limit = 0;
    coder.offset = 0;
    coder.len = 0;
}

// 创建解码器的函数
pub fn lzma_lzma_decoder_create(
    lz: &mut LzmaLzDecoder,

    options: &LzmaOptionsLzma,
    lz_options: &mut LzmaLzDecoderOptions,
) -> LzmaRet {
    if lz.coder.is_none() {
        lz.coder = Some(LzCoderType::LzmaDecoder(LzmaLzma1Decoder::default()));
        lz.code = Some(lzma_decode);
        lz.reset = Some(lzma_decoder_reset);
        lz.set_uncompressed = Some(lzma_decoder_uncompressed);
    }

    // 这里所有的字典大小都是可以的，LZ 解码器会处理特殊情况
    lz_options.dict_size = options.dict_size as usize;
    lz_options.preset_dict = options.preset_dict.clone().unwrap_or_default();
    lz_options.preset_dict_size = options.preset_dict_size as usize;

    LzmaRet::Ok
}

pub fn lzma_decoder_init(
    lz: &mut LzmaLzDecoder,

    id: LzmaVli,
    options: &LzmaOptionsType,
    lz_options: &mut LzmaLzDecoderOptions,
) -> LzmaRet {
    let tmp = options.as_lzma_options_lzma().unwrap();

    if !is_lclppb_valid(tmp) {
        return LzmaRet::ProgError;
    }

    let mut uncomp_size = LZMA_VLI_UNKNOWN;
    let mut allow_eopm = true;

    if id == LZMA_FILTER_LZMA1EXT {
        let opt = options.as_lzma_options_lzma().unwrap();

        // Only one flag is supported.
        if opt.ext_flags & !LZMA_LZMA1EXT_ALLOW_EOPM != 0 {
            return LzmaRet::OptionsError;
        }

        // 处理压缩文件的大小
        uncomp_size = opt.ext_size_low as u64 + ((opt.ext_size_high as u64) << 32);
        allow_eopm =
            (opt.ext_flags & LZMA_LZMA1EXT_ALLOW_EOPM != 0) || uncomp_size == LZMA_VLI_UNKNOWN;
    }

    // 调用创建解码器的函数
    // let tmp = options.downcast_mut::<LzmaOptionsLzma>().unwrap();
    let result = lzma_lzma_decoder_create(lz, tmp, lz_options);
    if result != LzmaRet::Ok {
        return result;
    }

    // 重置解码器并设置未压缩数据
    // let mut tmp: Box<dyn std::any::Any> = Box::new(options);
    lzma_decoder_reset(lz.coder.as_mut().unwrap(), options);
    lzma_decoder_uncompressed(lz.coder.as_mut().unwrap(), uncomp_size, allow_eopm);

    LzmaRet::Ok
}

pub fn lzma_lzma_decoder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    assert!(filters[1].init.is_none());
    // 报错位置
    lzma_lz_decoder_init(next, filters, lzma_decoder_init)
}

pub fn lzma_lzma_lclppb_decode(options: &mut LzmaOptionsLzma, byte: u8) -> bool {
    if byte > (4 * 5 + 4) * 9 + 8 {
        return true;
    }

    options.pb = (byte / (9 * 5)) as u32;
    let mut byte = (byte as u32 - options.pb * 9 * 5) as u32;
    options.lp = byte / 9;
    options.lc = byte - options.lp * 9;

    options.lc + options.lp > LZMA_LCLP_MAX
}

pub fn lzma_lzma_decoder_memusage_nocheck(options: &LzmaOptionsType) -> u64 {
    let opt = match options {
        LzmaOptionsType::LzmaOptionsLzma(options) => options,
        _ => return u64::MAX,
    };

    let opt = options.as_lzma_options_lzma().unwrap();

    std::mem::size_of::<LzmaLzma1Decoder>() as u64
        + lzma_lz_decoder_memusage(opt.dict_size as usize)
}

pub fn lzma_lzma_decoder_memusage(options: &LzmaOptionsType) -> u64 {
    let opt = options.as_lzma_options_lzma().unwrap();
    if !is_lclppb_valid(opt) {
        return u64::MAX;
    }

    lzma_lzma_decoder_memusage_nocheck(options)
}

// 解码 LZMA 属性的函数
pub fn lzma_lzma_props_decode(
    // options: &mut Option<Box<LzmaOptionsType>>,
    props: &[u8],
    props_size: usize,
) -> (LzmaRet, Option<LzmaOptionsType>) {
    // 检查属性大小是否为 5
    if props_size != 5 {
        return (LzmaRet::OptionsError, None);
    }

    // 分配 LzmaOptionsLzma 结构体
    let mut opt = LzmaOptionsLzma::default();
    if Some(opt.clone()).is_none() {
        return (LzmaRet::MemError, None);
    }

    let opt_clone = opt.clone();
    if lzma_lzma_lclppb_decode(&mut opt.clone(), props[0]) {
        return (LzmaRet::OptionsError, None);
    }

    // 设置字典大小
    opt.dict_size = read32le(&props[1..5]);

    // 设置预设字典为空
    opt.preset_dict = None;
    opt.preset_dict_size = 0;

    // 将解码后的选项赋值给 options
    let options = Some(LzmaOptionsType::LzmaOptionsLzma(opt.clone()));

    (LzmaRet::Ok, options)
}
