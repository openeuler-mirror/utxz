/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::rangecoder::{bit_reset, bittree_reset};
use crate::{
    api::{
        LzmaAction, LzmaMode, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaVli,
        LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA1EXT, LZMA_FILTER_LZMA2, LZMA_LZMA1EXT_ALLOW_EOPM,
    },
    common::{LzmaFilterInfo, LzmaNextCoder},
    get_dist_state,
    lz::{
        lzma_lz_encoder_init, lzma_lz_encoder_memusage, mf_get_hash_bytes, mf_position, mf_skip,
        LzEncoderType, LzmaLzEncoder, LzmaLzOptions, LzmaMf,
    },
    lzma::{
        get_dist_slot, is_lclppb_valid, literal_init, update_literal, ALIGN_BITS, ALIGN_MASK,
        DIST_MODEL_START, DIST_SLOT_BITS, LIT_STATES, MATCH_LEN_MIN, STATE_LIT_LIT,
    },
    rangecoder::{
        rc_bit, rc_bit_0_price, rc_bit_1_price, rc_bittree_price, rc_encode_dummy, rc_pending,
        LzmaRangeEncoder, Probability,
    },
};
use common::{my_max, write32le};
use std::sync::{Arc, Mutex};

use super::{
    is_literal_state, lzma_lzma_optimum_fast, lzma_lzma_optimum_normal, update_long_rep,
    update_match, update_short_rep, LzmaLengthEncoder, LzmaLzma1Encoder, ALIGN_SIZE,
    DIST_MODEL_END, DIST_SLOTS, DIST_STATES, FULL_DISTANCES, LEN_HIGH_BITS, LEN_HIGH_SYMBOLS,
    LEN_LOW_BITS, LEN_LOW_SYMBOLS, LEN_MID_BITS, LEN_MID_SYMBOLS, LEN_SYMBOLS, LITERAL_CODERS_MAX,
    LITERAL_CODER_SIZE, LZMA2_CHUNK_MAX, MATCH_LEN_MAX, POS_STATES_MAX, REPS, STATES,
};

#[macro_export]
macro_rules! not_equal_16 {
    ($a:expr, $b:expr) => {
        $a[0] != $b[0] || $a[1] != $b[1]
    };
}

struct LzmaOptimal {
    state: u32,
    prev_1_is_literal: bool,
    prev_2: bool,
    pos_prev_2: u32,
    back_prev_2: u32,
    price: u32,
    pos_prev: u32,
    back_prev: u32,
    backs: [u32; REPS],
}

const OPTS: usize = 1 << 12;

/////////////
// Literal //
/////////////
fn literal_matched(
    rc: &mut LzmaRangeEncoder,
    subcoder: &mut [Arc<Mutex<u16>>],
    match_byte: u32,
    mut symbol: u32,
) {
    let mut offset = 0x100;
    symbol += 1 << 8;

    let mut match_byte = match_byte;

    while symbol < (1 << 16) {
        match_byte <<= 1;
        let match_bit = match_byte & offset;
        let subcoder_index = (offset + match_bit + (symbol >> 8)) as usize;
        let bit = (symbol >> 7) & 1;

        rc_bit(rc, Arc::clone(&mut subcoder[subcoder_index]), bit);

        symbol <<= 1;
        offset &= !(match_byte ^ symbol);
    }
}

fn literal(coder: &mut LzmaLzma1Encoder, mf: &mut LzmaMf, position: u32) {
    // 定位要编码的字节和子编码器
    let cur_byte = mf.buffer[mf.read_pos as usize - mf.read_ahead as usize];

    let index = (((position) & (coder.literal_pos_mask)) << (coder.literal_context_bits))
        + ((mf.buffer[(mf.read_pos - mf.read_ahead - 1) as usize]) as u32
            >> (8 - (coder.literal_context_bits)));

    let mut subcoder = &mut (coder.literal)[index as usize];

    if is_literal_state(coder.state) {
        // 上一个 LZMA 符号是字面值。编码一个普通字面值，不使用匹配字节。
        coder.rc.rc_bittree(subcoder, 8, cur_byte as u32);
    } else {
        // 上一个 LZMA 符号是匹配。使用匹配的最后一个字节作为“匹配字节”。
        // 即，比较当前字面值和匹配字节的位。
        let match_byte =
            mf.buffer[mf.read_pos as usize - coder.reps[0] as usize - 1 - mf.read_ahead as usize];
        literal_matched(&mut coder.rc, subcoder, match_byte as u32, cur_byte as u32);
    }

    // 更新字面值状态
    coder.state = update_literal(coder.state);
}

//////////////////
// Match length //
//////////////////
fn length_update_prices(lc: &mut LzmaLengthEncoder, pos_state: u32) {
    let table_size = lc.table_size;
    lc.counters[pos_state as usize] = table_size;

    let a0 = rc_bit_0_price(lc.choice.lock().unwrap().clone());
    let a1 = rc_bit_1_price(lc.choice.lock().unwrap().clone());
    let b0 = a1 + rc_bit_0_price(lc.choice2.lock().unwrap().clone());
    let b1 = a1 + rc_bit_1_price(lc.choice2.lock().unwrap().clone());
    let prices = &mut lc.prices[pos_state as usize];

    let mut i = 0;
    // 注意 i的取值范围，
    // 处理 LEN_LOW_SYMBOLS 范围
    for i in 0..table_size.min(LEN_LOW_SYMBOLS as u32) {
        prices[i as usize] =
            a0 + rc_bittree_price(&lc.low[pos_state as usize], LEN_LOW_BITS as u32, i);
    }

    // 处理 LEN_MID_SYMBOLS 范围
    for i in
        LEN_LOW_SYMBOLS..table_size.min(LEN_LOW_SYMBOLS as u32 + LEN_MID_SYMBOLS as u32) as usize
    {
        prices[i as usize] = b0
            + rc_bittree_price(
                &lc.mid[pos_state as usize],
                LEN_MID_BITS as u32,
                i as u32 - LEN_LOW_SYMBOLS as u32,
            );
    }

    // 处理剩余的部分
    for i in (LEN_LOW_SYMBOLS + LEN_MID_SYMBOLS)..table_size as usize {
        prices[i as usize] = b1
            + rc_bittree_price(
                &lc.high,
                LEN_HIGH_BITS as u32,
                i as u32 - LEN_LOW_SYMBOLS as u32 - LEN_MID_SYMBOLS as u32,
            );
    }
}

fn length(
    rc: &mut LzmaRangeEncoder,
    lc: &mut LzmaLengthEncoder,
    pos_state: u32,
    mut len: u32,
    fast_mode: bool,
) {
    assert!(len <= MATCH_LEN_MAX as u32);
    len -= MATCH_LEN_MIN as u32;

    if len < LEN_LOW_SYMBOLS as u32 {
        rc_bit(rc, Arc::clone(&lc.choice), 0);

        rc.rc_bittree(&mut lc.low[pos_state as usize], LEN_LOW_BITS as u32, len);
    } else {
        rc_bit(rc, Arc::clone(&lc.choice), 1);
        len -= LEN_LOW_SYMBOLS as u32;

        if len < LEN_MID_SYMBOLS as u32 {
            rc_bit(rc, Arc::clone(&lc.choice2), 0);
            rc.rc_bittree(&mut lc.mid[pos_state as usize], LEN_MID_BITS as u32, len);
        } else {
            rc_bit(rc, Arc::clone(&lc.choice2), 1);
            len -= LEN_MID_SYMBOLS as u32;
            rc.rc_bittree(&mut lc.high, LEN_HIGH_BITS as u32, len);
        }
    }
    if !fast_mode {
        lc.counters[pos_state as usize] = lc.counters[pos_state as usize].saturating_sub(1);

        if lc.counters[pos_state as usize] == 0 {
            length_update_prices(lc, pos_state);
        }
    }
}

///////////
// Match //
///////////
fn match_lzma(coder: &mut LzmaLzma1Encoder, pos_state: u32, distance: u32, len: u32) {
    update_match(&mut coder.state);

    length(
        &mut coder.rc,
        &mut coder.match_len_encoder,
        pos_state,
        len,
        coder.fast_mode,
    );

    let dist_slot = get_dist_slot(distance);
    let dist_state = get_dist_state!(len);
    coder.rc.rc_bittree(
        &mut coder.dist_slot[dist_state as usize],
        DIST_SLOT_BITS as u32,
        dist_slot,
    );

    if dist_slot >= DIST_MODEL_START as u32 {
        let footer_bits = (dist_slot >> 1) - 1;
        let base = (2 | (dist_slot & 1)) << footer_bits;
        let dist_reduced = distance - base;

        // Careful here: base - dist_slot - 1 can be -1, but
        // rc_bittree_reverse starts at probs[1], not probs[0].
        let mut flags: u8 = 0;
        if dist_slot < DIST_MODEL_END as u32 {
            if (base - dist_slot) < 1 {
                flags = 1;
                coder.rc.rc_bittree_reverse(
                    &mut coder.dist_special[(0) as usize..],
                    footer_bits,
                    dist_reduced,
                    flags,
                );
            } else {
                coder.rc.rc_bittree_reverse(
                    &mut coder.dist_special[(base - dist_slot - 1) as usize..],
                    footer_bits,
                    dist_reduced,
                    flags,
                );
            }
        } else {
            coder
                .rc
                .rc_direct(dist_reduced >> ALIGN_BITS, footer_bits - ALIGN_BITS as u32);
            coder.rc.rc_bittree_reverse(
                &mut coder.dist_align,
                ALIGN_BITS as u32,
                dist_reduced & ALIGN_MASK as u32,
                flags,
            );
            coder.align_price_count += 1;
        }
    }

    coder.reps[3] = coder.reps[2];
    coder.reps[2] = coder.reps[1];
    coder.reps[1] = coder.reps[0];
    coder.reps[0] = distance;
    coder.match_price_count += 1;
}

////////////////////
// Repeated match //
////////////////////

fn rep_match(coder: &mut LzmaLzma1Encoder, pos_state: u32, rep: u32, len: u32) {
    if rep == 0 {
        rc_bit(
            &mut coder.rc,
            Arc::clone(&coder.is_rep0[coder.state as usize]),
            0,
        );
        rc_bit(
            &mut coder.rc,
            Arc::clone(&coder.is_rep0_long[coder.state as usize][pos_state as usize]),
            (len != 1) as u32,
        );
    } else {
        let distance = coder.reps[rep as usize];
        rc_bit(
            &mut coder.rc,
            Arc::clone(&coder.is_rep0[coder.state as usize]),
            1,
        );

        if rep == 1 {
            rc_bit(
                &mut coder.rc,
                Arc::clone(&coder.is_rep1[coder.state as usize]),
                0,
            );
        } else {
            rc_bit(
                &mut coder.rc,
                Arc::clone(&coder.is_rep1[coder.state as usize]),
                1,
            );
            rc_bit(
                &mut coder.rc,
                Arc::clone(&coder.is_rep2[coder.state as usize]),
                rep - 2,
            );

            if rep == 3 {
                coder.reps[3] = coder.reps[2];
            }

            coder.reps[2] = coder.reps[1];
        }

        coder.reps[1] = coder.reps[0];
        coder.reps[0] = distance;
    }

    if len == 1 {
        update_short_rep(&mut coder.state);
    } else {
        // println!("rep match       length");
        length(
            &mut coder.rc,
            &mut coder.rep_len_encoder,
            pos_state,
            len,
            coder.fast_mode,
        );
        update_long_rep(&mut coder.state);
    }
}

//////////
// Main //
//////////

fn encode_symbol(
    coder: &mut LzmaLzma1Encoder,
    mf: &mut LzmaMf,
    back: u32,
    len: u32,
    position: u32,
) {
    let pos_state = position & coder.pos_mask;

    if back == u32::MAX {
        // 字面值，即八位字节
        assert!(len == 1);
        rc_bit(
            &mut coder.rc,
            Arc::clone(&coder.is_match[coder.state as usize][pos_state as usize]),
            0,
        );
        literal(coder, mf, position);
    } else {
        // 某种类型的匹配
        rc_bit(
            &mut coder.rc,
            Arc::clone(&coder.is_match[coder.state as usize][pos_state as usize]),
            1,
        );

        if back < REPS as u32 {
            // 这是一个重复匹配，即之前使用过相同的距离。
            rc_bit(
                &mut coder.rc,
                Arc::clone(&coder.is_rep[coder.state as usize]),
                1,
            );
            rep_match(coder, pos_state, back, len);
        } else {
            // 正常匹配
            rc_bit(
                &mut coder.rc,
                Arc::clone(&coder.is_rep[coder.state as usize]),
                0,
            );
            match_lzma(coder, pos_state, back - REPS as u32, len);
        }
    }

    assert!(mf.read_ahead >= len);
    mf.read_ahead -= len;
}

fn encode_init(coder: &mut LzmaLzma1Encoder, mf: &mut LzmaMf) -> bool {
    // 确保匹配查找器的位置为0
    assert!(mf_position(mf) == 0);
    // 确保未压缩数据的大小为0
    assert!(coder.uncomp_size == 0);

    if mf.read_pos == mf.read_limit {
        // 如果没有更多数据可供读取
        if mf.action == LzmaAction::Run {
            return false; // 无法进行任何操作
        }

        // 正在结束编码（在刷新时不会到达这里）
        assert!(mf.write_pos == mf.read_pos);
        assert!(mf.action == LzmaAction::Finish);
    } else {
        // 执行实际初始化。第一个LZMA符号必须始终是字面值。
        mf_skip(mf, 1);
        mf.read_ahead = 0;
        // rc_bit(&mut coder.rc, Arc::clone(&coder.is_match[0][0]), 0);
        coder.rc.rc_bit(Arc::clone(&coder.is_match[0][0]), 0);
        coder
            .rc
            .rc_bittree(&mut coder.literal[0], 8, mf.buffer[0] as u32);
        coder.uncomp_size += 1;
    }

    // 初始化完成（如果不是空文件）
    coder.is_initialized = true;

    true
}

fn encode_eopm(coder: &mut LzmaLzma1Encoder, position: u32) {
    let pos_state = position & coder.pos_mask;
    rc_bit(
        &mut coder.rc,
        Arc::clone(&coder.is_match[coder.state as usize][pos_state as usize]),
        1,
    );
    rc_bit(
        &mut coder.rc,
        Arc::clone(&coder.is_rep[coder.state as usize]),
        0,
    );
    match_lzma(coder, pos_state, u32::MAX, MATCH_LEN_MIN as u32);
}

pub const LOOP_INPUT_MAX: usize = OPTS + 1;

pub fn lzma_lzma_encode(
    coder: &mut LzmaLzma1Encoder,
    mf: &mut LzmaMf,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    limit: u32,
) -> LzmaRet {
    // println!("============ lzma_lzma_encode");
    // 如果没有数据被编码，初始化流。
    if !coder.is_initialized && !encode_init(coder, mf) {
        return LzmaRet::Ok;
    }

    // 编码范围编码器中的待处理输出字节
    if coder.rc.rc_encode(out, out_pos, out_size) {
        assert!(limit == u32::MAX);
        return LzmaRet::Ok;
    }

    if coder.is_flushed {
        assert!(limit == u32::MAX);
        return LzmaRet::StreamEnd;
    }
    // println!("mf {:#?}", mf);
    let mut len: u32 = 0;
    let mut back: u32 = 0;
    loop {
        if limit != u32::MAX
            && (mf.read_pos - mf.read_ahead >= limit
                || *out_pos + rc_pending(&coder.rc) as usize
                    >= LZMA2_CHUNK_MAX as usize - LOOP_INPUT_MAX)
        {
            break;
        }

        if mf.read_pos >= mf.read_limit {
            if mf.action == LzmaAction::Run {
                return LzmaRet::Ok;
            }

            if mf.read_ahead == 0 {
                break;
            }
        }

        if coder.fast_mode {
            lzma_lzma_optimum_fast(coder, mf, &mut back, &mut len)
        } else {
            lzma_lzma_optimum_normal(coder, mf, &mut back, &mut len, coder.uncomp_size as u32)
        };

        encode_symbol(coder, mf, back, len, coder.uncomp_size as u32);

        if coder.out_limit != 0 && rc_encode_dummy(&mut coder.rc, coder.out_limit) {
            coder.rc.rc_forget();
            break;
        }

        coder.uncomp_size += len as u64;

        if coder.rc.rc_encode(out, out_pos, out_size) {
            assert!(limit == u32::MAX);
            return LzmaRet::Ok;
        }
    }

    if !coder.uncomp_size_ptr.is_none() {
        coder.uncomp_size_ptr = Some(coder.uncomp_size);
    }

    if coder.use_eopm {
        encode_eopm(coder, coder.uncomp_size as u32);
    }

    coder.rc.rc_flush();

    if coder.rc.rc_encode(out, out_pos, out_size) {
        assert!(limit == u32::MAX);
        coder.is_flushed = true;
        return LzmaRet::Ok;
    }

    LzmaRet::StreamEnd
}

fn lzma_encode(
    coder: &mut LzEncoderType,
    mf: &mut LzmaMf,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    if mf.action == LzmaAction::SyncFlush {
        return LzmaRet::OptionsError;
    }

    // let coder = coder.downcast_mut::<LzmaLzma1Encoder>().unwrap();
    let coder = match coder {
        LzEncoderType::LzmaEncoderPrivate(coder) => coder,
        _ => panic!("Invalid coder type"),
    };
    lzma_lzma_encode(coder, mf, out, out_pos, out_size, u32::MAX)
}

fn lzma_lzma_set_out_limit(
    coder_ptr: &mut LzEncoderType,
    uncomp_size: &mut u64,
    out_limit: u64,
) -> LzmaRet {
    // 最小输出大小为 5 字节，但不能容纳任何输出，因此我们使用 6 字节。
    if out_limit < 6 {
        return LzmaRet::BufError;
    }

    // let coder = coder_ptr.downcast_mut::<LzmaLzma1Encoder>().unwrap();
    let coder = match coder_ptr {
        LzEncoderType::LzmaEncoderPrivate(coder) => coder,
        _ => panic!("Invalid coder type"),
    };
    coder.out_limit = out_limit;
    coder.uncomp_size_ptr = Some(*uncomp_size);
    coder.use_eopm = false;
    LzmaRet::Ok
}

////////////////////
// Initialization //
////////////////////

fn is_options_valid(options: &LzmaOptionsLzma) -> bool {
    // 验证一些选项。LZ 编码器也验证 nice_len，但我们需要在此处提前验证。
    is_lclppb_valid(options)
        && options.nice_len >= MATCH_LEN_MIN as u32
        && options.nice_len <= MATCH_LEN_MAX as u32
        && (options.mode == LzmaMode::Fast || options.mode == LzmaMode::Normal)
}

fn set_lz_options(lz_options: &mut LzmaLzOptions, options: &LzmaOptionsLzma) {
    // LZ 编码器初始化会验证这些选项，因此我们不需要在这里验证。
    lz_options.before_size = OPTS;
    lz_options.dict_size = options.dict_size as usize;
    lz_options.after_size = LOOP_INPUT_MAX;
    lz_options.match_len_max = MATCH_LEN_MAX;
    lz_options.nice_len = my_max(
        mf_get_hash_bytes(options.mf.clone()) as usize,
        options.nice_len as usize,
    );
    lz_options.match_finder = options.mf.clone();
    lz_options.depth = options.depth;
    lz_options.preset_dict = options.preset_dict.clone();
    lz_options.preset_dict_size = options.preset_dict_size;
}

fn length_encoder_reset(lencoder: &mut LzmaLengthEncoder, num_pos_states: u32, fast_mode: bool) {
    // 重置 choice 和 choice2 位
    bit_reset(Arc::clone(&lencoder.choice));
    bit_reset(Arc::clone(&lencoder.choice2));

    // 重置每个 pos_state 的低位和中位比特树
    for pos_state in 0..num_pos_states {
        bittree_reset(&mut lencoder.low[pos_state as usize], LEN_LOW_BITS);
        bittree_reset(&mut lencoder.mid[pos_state as usize], LEN_MID_BITS);
    }

    // 重置高位比特树
    bittree_reset(&mut lencoder.high, LEN_HIGH_BITS);

    // 如果不是 fast_mode，更新价格
    if !fast_mode {
        for pos_state in 0..num_pos_states {
            length_update_prices(lencoder, pos_state);
        }
    }
}

pub fn lzma_lzma_encoder_reset(coder: &mut LzmaLzma1Encoder, options: &LzmaOptionsLzma) -> LzmaRet {
    if !is_options_valid(options) {
        return LzmaRet::OptionsError;
    }

    coder.pos_mask = (1 << options.pb) - 1;
    coder.literal_context_bits = options.lc;
    coder.literal_pos_mask = (1 << options.lp) - 1;

    // 范围编码器重置
    coder.rc.rc_reset();

    // 状态初始化
    coder.state = STATE_LIT_LIT;
    for i in 0..REPS {
        coder.reps[i] = 0;
    }

    literal_init(&mut *coder.literal, options.lc, options.lp);

    // 比特编码器重置
    for i in 0..STATES {
        for j in 0..=coder.pos_mask as usize {
            bit_reset(Arc::clone(&coder.is_match[i][j]));
            bit_reset(Arc::clone(&coder.is_rep0_long[i][j]));
        }

        bit_reset(Arc::clone(&coder.is_rep[i]));
        bit_reset(Arc::clone(&coder.is_rep0[i]));
        bit_reset(Arc::clone(&coder.is_rep1[i]));
        bit_reset(Arc::clone(&coder.is_rep2[i]));
    }

    for i in 0..FULL_DISTANCES - DIST_MODEL_END {
        bit_reset(Arc::clone(&coder.dist_special[i]));
    }

    // 比特树编码器重置
    for i in 0..DIST_STATES {
        bittree_reset(&mut coder.dist_slot[i], DIST_SLOT_BITS);
    }

    bittree_reset(&mut coder.dist_align, ALIGN_BITS);

    // 长度编码器重置
    length_encoder_reset(
        &mut coder.match_len_encoder,
        1 << options.pb,
        coder.fast_mode,
    );

    length_encoder_reset(&mut coder.rep_len_encoder, 1 << options.pb, coder.fast_mode);

    // 价格计数初始化
    coder.match_price_count = u32::MAX / 2;
    coder.align_price_count = u32::MAX / 2;

    coder.opts_end_index = 0;
    coder.opts_current_index = 0;

    LzmaRet::Ok
}

pub fn lzma_lzma_encoder_create(
    coder_ptr: Option<&mut LzEncoderType>,
    id: LzmaVli,
    options: &LzmaOptionsLzma,
    lz_options: &mut LzmaLzOptions,
) -> LzmaRet {
    assert!(id == LZMA_FILTER_LZMA1 || id == LZMA_FILTER_LZMA1EXT || id == LZMA_FILTER_LZMA2);

    let coder = &mut match coder_ptr {
        Some(coder_) => match coder_ {
            LzEncoderType::LzmaEncoderPrivate(t) => t,
            _ => panic!("Invalid coder type"),
        },
        None => &mut LzmaLzma1Encoder::new(),
    };
    // let mut coder: Box<LzmaLzma1Encoder> = match coder_ptr {
    //     Some(coder_) => match coder_ {
    //         LzEncoderType::LzmaEncoderPrivate(t) => Box::new(t.to_owned()),
    //         _ => panic!("Invalid coder type"),
    //     },
    //     None => Box::new(LzmaLzma1Encoder::default()),
    // };

    // 设置压缩模式。注意，我们尚未验证选项。无效选项将在函数末尾的 lzma_lzma_encoder_reset() 调用中被拒绝。
    match options.mode {
        LzmaMode::Fast => {
            coder.fast_mode = true;
        }
        LzmaMode::Normal => {
            coder.fast_mode = false;

            // 设置 dist_table_size。
            // 将字典大小向上舍入到下一个 2^n。
            if options.dict_size > (1 << 30) + (1 << 29) {
                return LzmaRet::OptionsError;
            }

            let mut log_size = 0;
            while (1 << log_size) < options.dict_size {
                log_size += 1;
            }

            coder.dist_table_size = log_size * 2;

            // 长度编码器的价格表大小
            let nice_len = my_max(mf_get_hash_bytes(options.mf.clone()), options.nice_len);

            coder.match_len_encoder.table_size = nice_len + 1 - MATCH_LEN_MIN as u32;

            coder.rep_len_encoder.table_size = nice_len + 1 - MATCH_LEN_MIN as u32;
        }
        _ => {
            return LzmaRet::OptionsError;
        }
    }

    // 如果有非空的预设字典，则不需要将第一个字节写为字面值。
    coder.is_initialized = options.preset_dict.is_some() && options.preset_dict_size > 0;
    coder.is_flushed = false;
    coder.uncomp_size = 0;
    coder.uncomp_size_ptr = None;

    // 默认情况下禁用输出大小限制。
    coder.out_limit = 0;

    // 确定是否需要结束标记：
    // - LZMA2 从不使用它。
    // - LZMA_FILTER_LZMA1 始终使用它（除非稍后调用 lzma_lzma_set_out_limit()）。
    // - LZMA_FILTER_LZMA1EXT 在选项中有一个标志。
    coder.use_eopm = id == LZMA_FILTER_LZMA1;
    if id == LZMA_FILTER_LZMA1EXT {
        // 检查是否存在不支持的标志。
        if options.ext_flags & !LZMA_LZMA1EXT_ALLOW_EOPM != 0 {
            return LzmaRet::OptionsError;
        }

        coder.use_eopm = (options.ext_flags & LZMA_LZMA1EXT_ALLOW_EOPM) != 0;
    }

    set_lz_options(lz_options, options);

    lzma_lzma_encoder_reset(coder, options)
}

pub fn lzma_lzma_encoder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    lzma_lz_encoder_init(next, filters, lzma_encoder_init)
}

pub fn lzma_encoder_init(
    lz: &mut LzmaLzEncoder,
    id: LzmaVli,
    options: &LzmaOptionsType,
    lz_options: &mut LzmaLzOptions,
) -> LzmaRet {
    lz.code = Some(lzma_encode);
    lz.set_out_limit = Some(lzma_lzma_set_out_limit);
    let options = options.as_lzma_options_lzma().unwrap();
    lzma_lzma_encoder_create(Some(lz.coder.as_mut().unwrap()), id, options, lz_options)
}

pub fn lzma_lzma_encoder_memusage(options: &LzmaOptionsType) -> u64 {
    let opt = match options {
        LzmaOptionsType::LzmaOptionsLzma(c) => c,
        _ => return u64::MAX,
    };

    if !is_options_valid(opt) {
        return u64::MAX;
    }

    let mut lz_options = LzmaLzOptions::default();
    set_lz_options(&mut lz_options, opt);

    let lz_memusage = lzma_lz_encoder_memusage(&lz_options);
    if lz_memusage == u64::MAX {
        return u64::MAX;
    }

    std::mem::size_of::<LzmaLzma1Encoder>() as u64 + lz_memusage
}

// pub fn lzma_lzma_lclppb_encode(options: &LzmaOptionsLzma, byte: *mut u8) -> bool {
//     if !is_lclppb_valid(options) {
//         return true;
//     }

//     unsafe {
//         *byte = ((options.pb * 5 + options.lp) * 9 + options.lc) as u8 ;
//         assert!(*byte <= (4 * 5 + 4) * 9 + 8);
//     }

//     false
// }

pub fn lzma_lzma_lclppb_encode(options: &LzmaOptionsLzma, byte: &mut [u8]) -> bool {
    if !is_lclppb_valid(options) {
        return true;
    }

    byte[0] = ((options.pb * 5 + options.lp) * 9 + options.lc) as u8;
    assert!(byte[0] <= (4 * 5 + 4) * 9 + 8);

    false
}

pub fn lzma_lzma_props_encode(options: &LzmaOptionsType, out: &mut [u8]) -> LzmaRet {
    if Some(options).is_none() {
        return LzmaRet::ProgError;
    }

    // let opt = &mut LzmaOptionsLzma::default();
    let opt = match options {
        LzmaOptionsType::LzmaOptionsLzma(c) => c,
        _ => return LzmaRet::ProgError,
    };
    // let opt = options.unwrap();
    if lzma_lzma_lclppb_encode(opt, out) {
        return LzmaRet::OptionsError;
    }

    write32le(&mut out[1..], opt.dict_size);

    return LzmaRet::Ok;
}

pub fn lzma_mode_is_supported(mode: LzmaMode) -> bool {
    mode == LzmaMode::Fast || mode == LzmaMode::Normal
}
