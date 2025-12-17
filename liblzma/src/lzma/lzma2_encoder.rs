/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_max;
use std::mem;

use crate::{
    api::{
        LzmaAction, LzmaFilter, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaVli,
        LZMA_DICT_SIZE_MIN, LZMA_FILTER_LZMA2, LZMA_LCLP_MAX, LZMA_PB_MAX,
    },
    common::{lzma_bufcpy, LzmaFilterInfo, LzmaNextCoder},
    lz::{
        lzma_lz_encoder_init, mf_read, mf_unencoded, LzEncoderType, LzmaLzDecoder, LzmaLzEncoder,
        LzmaLzOptions, LzmaMf,
    },
    lzma::{
        lzma_lzma_encode, lzma_lzma_encoder_create, lzma_lzma_encoder_reset,
        lzma_lzma_lclppb_encode, LzmaLzma1Encoder,
    },
};

use super::{get_dist_slot, lzma_lzma_encoder_memusage};

/// 最大的每个块的实际数据字节数（不包括头部）
pub const LZMA2_CHUNK_MAX: u32 = 1 << 16;

/// LZMA 块的最大未压缩大小（不包括头部）
pub const LZMA2_UNCOMPRESSED_MAX: u32 = 1 << 21;

/// LZMA2 头部的最大大小
pub const LZMA2_HEADER_MAX: usize = 6;

/// 未压缩块的头部大小
pub const LZMA2_HEADER_UNCOMPRESSED: usize = 3;

/// LZMA2 编码器结构体
#[derive(Debug)]
pub struct LzmaLzma2Encoder {
    /// 编码器的状态序列
    sequence: Sequence,

    /// LZMA 编码器
    lzma: Box<LzmaLzma1Encoder>,

    /// 当前使用的 LZMA 选项
    opt_cur: LzmaOptionsLzma,

    /// 是否需要属性
    need_properties: bool,

    /// 是否需要状态重置
    need_state_reset: bool,

    /// 是否需要字典重置
    need_dictionary_reset: bool,

    /// 未压缩块的大小
    uncompressed_size: usize,

    /// 压缩块的大小（不包括头部）
    compressed_size: usize,

    /// buf[] 中的读取位置
    buf_pos: usize,

    /// 用于存储块头和 LZMA 压缩数据的缓冲区
    buf: [u8; LZMA2_HEADER_MAX + LZMA2_CHUNK_MAX as usize],
}

impl Default for LzmaLzma2Encoder {
    fn default() -> Self {
        LzmaLzma2Encoder {
            sequence: Sequence::default(),           // 假设 Sequence 实现了 Default
            lzma: Box::new(LzmaLzma1Encoder::new()), // 假设 LzmaLzDecoder 实现了 Default
            opt_cur: LzmaOptionsLzma::default(),     // 假设 LzmaOptionsLzma 实现了 Default
            need_properties: false,
            need_state_reset: false,
            need_dictionary_reset: false,
            uncompressed_size: 0,
            compressed_size: 0,
            buf_pos: 0,
            buf: [0; LZMA2_HEADER_MAX + LZMA2_CHUNK_MAX as usize], // 假设 LZMA2_HEADER_MAX 和 LZMA2_CHUNK_MAX 是常量
        }
    }
}

/// 编码器的状态序列
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum Sequence {
    #[default]
    Init,
    LzmaEncode,
    LzmaCopy,
    UncompressedHeader,
    UncompressedCopy,
}

fn lzma2_header_lzma(coder: &mut LzmaLzma2Encoder) {
    assert!(coder.uncompressed_size > 0);
    assert!(coder.uncompressed_size <= LZMA2_UNCOMPRESSED_MAX as usize);
    assert!(coder.compressed_size > 0);
    assert!(coder.compressed_size <= LZMA2_CHUNK_MAX as usize);

    let mut pos: usize;

    if coder.need_properties {
        pos = 0;

        if coder.need_dictionary_reset {
            coder.buf[pos] = 0x80 + (3 << 5);
        } else {
            coder.buf[pos] = 0x80 + (2 << 5);
        }
    } else {
        pos = 1;

        if coder.need_state_reset {
            coder.buf[pos] = 0x80 + (1 << 5);
        } else {
            coder.buf[pos] = 0x80;
        }
    }

    // 设置复制的起始位置
    coder.buf_pos = pos;

    // 未压缩大小
    let mut size = coder.uncompressed_size - 1;
    coder.buf[pos] += (size >> 16) as u8;
    pos += 1;
    coder.buf[pos] = ((size >> 8) & 0xFF) as u8;
    pos += 1;
    coder.buf[pos] = (size & 0xFF) as u8;
    pos += 1;

    // 压缩大小
    size = coder.compressed_size - 1;
    coder.buf[pos] = (size >> 8) as u8;
    pos += 1;
    coder.buf[pos] = (size & 0xFF) as u8;
    pos += 1;

    // 如果需要，设置属性
    if coder.need_properties {
        lzma_lzma_lclppb_encode(&coder.opt_cur, &mut coder.buf[pos..]);
    }

    coder.need_properties = false;
    coder.need_state_reset = false;
    coder.need_dictionary_reset = false;

    // 复制代码使用 coder.compressed_size 来指示 coder.buf[] 的结束，
    // 因此我们需要在这里添加头部的最大大小。
    coder.compressed_size += LZMA2_HEADER_MAX;
}

fn lzma2_header_uncompressed(coder: &mut LzmaLzma2Encoder) {
    assert!(coder.uncompressed_size > 0);
    assert!(coder.uncompressed_size <= LZMA2_CHUNK_MAX as usize);

    // 如果这是第一个块，我们需要包含字典重置指示符。
    if coder.need_dictionary_reset {
        coder.buf[0] = 1;
    } else {
        coder.buf[0] = 2;
    }

    coder.need_dictionary_reset = false;

    // "压缩"大小
    coder.buf[1] = ((coder.uncompressed_size - 1) >> 8) as u8;
    coder.buf[2] = ((coder.uncompressed_size - 1) & 0xFF) as u8;

    // 设置复制的起始位置。
    coder.buf_pos = 0;
}

fn lzma2_encode(
    coder_ptr: &mut LzEncoderType,
    mf: &mut LzmaMf,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_mut::<LzmaLzma2Encoder>().unwrap();
    let coder = match coder_ptr {
        LzEncoderType::Lzma2Encoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    while *out_pos < out_size {
        match coder.sequence {
            Sequence::Init => {
                if mf_unencoded(mf) == 0 {
                    if mf.action == LzmaAction::Finish {
                        out[*out_pos] = 0;
                        *out_pos += 1;
                    }
                    return if mf.action == LzmaAction::Run {
                        LzmaRet::Ok
                    } else {
                        LzmaRet::StreamEnd
                    };
                }

                if coder.need_state_reset {
                    let ret = lzma_lzma_encoder_reset(&mut *coder.lzma, &coder.opt_cur);
                    if ret != LzmaRet::Ok {
                        return ret;
                    }
                }

                coder.uncompressed_size = 0;
                coder.compressed_size = 0;
                coder.sequence = Sequence::LzmaEncode;
            }

            Sequence::LzmaEncode => {
                let left = LZMA2_UNCOMPRESSED_MAX - coder.uncompressed_size as u32;
                let limit = if left < mf.match_len_max {
                    0
                } else {
                    mf.read_pos - mf.read_ahead + left - mf.match_len_max
                };

                let read_start = mf.read_pos - mf.read_ahead;

                let ret = lzma_lzma_encode(
                    &mut *coder.lzma,
                    mf,
                    &mut coder.buf[LZMA2_HEADER_MAX..],
                    &mut coder.compressed_size,
                    LZMA2_CHUNK_MAX as usize,
                    limit,
                );

                coder.uncompressed_size += (mf.read_pos - mf.read_ahead - read_start) as usize;

                assert!(coder.compressed_size <= LZMA2_CHUNK_MAX as usize);
                assert!(coder.uncompressed_size <= LZMA2_UNCOMPRESSED_MAX as usize);

                if ret != LzmaRet::StreamEnd {
                    return LzmaRet::Ok;
                }

                if coder.compressed_size >= coder.uncompressed_size {
                    coder.uncompressed_size += mf.read_ahead as usize;
                    assert!(coder.uncompressed_size <= LZMA2_UNCOMPRESSED_MAX as usize);
                    mf.read_ahead = 0;
                    lzma2_header_uncompressed(coder);
                    coder.need_state_reset = true;
                    coder.sequence = Sequence::UncompressedHeader;
                    continue;
                }

                lzma2_header_lzma(coder);
                coder.sequence = Sequence::LzmaCopy;
            }

            Sequence::LzmaCopy => {
                lzma_bufcpy(
                    &mut coder.buf,
                    &mut coder.buf_pos,
                    coder.compressed_size,
                    out,
                    out_pos,
                    out_size,
                );
                if coder.buf_pos != coder.compressed_size {
                    return LzmaRet::Ok;
                }

                coder.sequence = Sequence::Init;
            }

            Sequence::UncompressedHeader => {
                lzma_bufcpy(
                    &mut coder.buf,
                    &mut coder.buf_pos,
                    LZMA2_HEADER_UNCOMPRESSED,
                    out,
                    out_pos,
                    out_size,
                );
                if coder.buf_pos != LZMA2_HEADER_UNCOMPRESSED {
                    return LzmaRet::Ok;
                }

                coder.sequence = Sequence::UncompressedCopy;
            }

            Sequence::UncompressedCopy => {
                mf_read(mf, out, out_pos, out_size, &mut coder.uncompressed_size);
                if coder.uncompressed_size != 0 {
                    return LzmaRet::Ok;
                }

                coder.sequence = Sequence::Init;
            }
        }
    }

    LzmaRet::Ok
}

fn lzma2_encoder_end(coder_ptr: &mut LzEncoderType) {
    let coder = match coder_ptr {
        LzEncoderType::Lzma2Encoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
}

fn lzma2_encoder_options_update(coder_ptr: &mut LzEncoderType, filter: &LzmaFilter) -> LzmaRet {
    // let coder = coder_ptr.downcast_mut::<LzmaLzma2Encoder>().unwrap();
    let coder = match coder_ptr {
        LzEncoderType::Lzma2Encoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    // 只有当当前没有未完成的块时，才能更新选项
    if Some(filter.options.clone()).is_none() || coder.sequence != Sequence::Init {
        return LzmaRet::ProgError;
    }

    // 获取新选项
    // 获取新选项
    let opt = match filter.options.as_ref() {
        Some(LzmaOptionsType::LzmaOptionsLzma(temp)) => temp,
        _ => return LzmaRet::OptionsError,
    };

    // 仅允许修改 lc/lp/pb 参数
    if coder.opt_cur.lc != opt.lc || coder.opt_cur.lp != opt.lp || coder.opt_cur.pb != opt.pb {
        // 验证选项是否合法
        if opt.lc > LZMA_LCLP_MAX
            || opt.lp > LZMA_LCLP_MAX
            || opt.lc + opt.lp > LZMA_LCLP_MAX
            || opt.pb > LZMA_PB_MAX
        {
            return LzmaRet::OptionsError;
        }

        // 在编码器开始新的 LZMA2 块时应用新选项
        coder.opt_cur.lc = opt.lc;
        coder.opt_cur.lp = opt.lp;
        coder.opt_cur.pb = opt.pb;
        coder.need_properties = true;
        coder.need_state_reset = true;
    }

    LzmaRet::Ok
}

fn lzma2_encoder_init(
    lz: &mut LzmaLzEncoder,
    _id: LzmaVli,
    options: &LzmaOptionsType,
    lz_options: &mut LzmaLzOptions,
) -> LzmaRet {
    if Some(options).is_none() {
        return LzmaRet::ProgError;
    }

    let mut coder = &mut LzmaLzma2Encoder::default();
    if lz.coder.is_none() {
        let coder_ = LzmaLzma2Encoder::default();
        lz.coder = Some(LzEncoderType::Lzma2Encoder(coder_));
        lz.code = Some(lzma2_encode);
        lz.end = Some(lzma2_encoder_end);
        lz.options_update = Some(lzma2_encoder_options_update);
        coder.lzma = Box::new(LzmaLzma1Encoder::new());
    }
    coder = match lz.coder {
        Some(LzEncoderType::Lzma2Encoder(ref mut c)) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.opt_cur = options.as_lzma_options_lzma().unwrap().clone();

    coder.sequence = Sequence::Init;
    coder.need_properties = true;
    coder.need_state_reset = false;
    coder.need_dictionary_reset =
        coder.opt_cur.preset_dict.is_none() || coder.opt_cur.preset_dict_size == 0;

    // 先取出 coder.lzma 的内容，使用默认值替换（要求 LzmaLzma1Encoder 实现 Default）
    // let mut encoder = LzEncoderType::LzmaEncoderPrivate(mem::take(&mut coder.lzma));
    let mut encoder = LzEncoderType::LzmaEncoderPrivate(*coder.lzma.clone());
    // 调用 lzma_lzma_encoder_create，将临时 encoder 的可变引用传入
    let ret = lzma_lzma_encoder_create(
        Some(&mut encoder),
        LZMA_FILTER_LZMA2,
        &coder.opt_cur,
        lz_options,
    );

    // 将 encoder 中更新后的内部值放回 coder.lzma
    if let LzEncoderType::LzmaEncoderPrivate(inner) = encoder {
        coder.lzma = Box::new(inner);
    } else {
        unreachable!();
    }

    if ret != LzmaRet::Ok {
        return ret;
    }

    if lz_options.before_size + lz_options.dict_size < LZMA2_CHUNK_MAX as usize {
        lz_options.before_size = LZMA2_CHUNK_MAX as usize - lz_options.dict_size;
    }

    LzmaRet::Ok
}

pub fn lzma_lzma2_encoder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    lzma_lz_encoder_init(next, filters, lzma2_encoder_init)
}

pub fn lzma_lzma2_encoder_memusage(options: &LzmaOptionsType) -> u64 {
    let lzma_mem = lzma_lzma_encoder_memusage(options);
    if lzma_mem == u64::MAX {
        return u64::MAX;
    }

    std::mem::size_of::<LzmaLzma2Encoder>() as u64 + lzma_mem
}

pub fn lzma_lzma2_props_encode(options: &LzmaOptionsType, out: &mut [u8]) -> LzmaRet {
    if Some(options).is_none() {
        return LzmaRet::ProgError;
    }

    // let opt = &mut LzmaOptionsLzma::default();
    let opt = match options {
        LzmaOptionsType::LzmaOptionsLzma(c) => c,
        _ => return LzmaRet::ProgError,
    };
    let mut d = my_max(opt.dict_size, LZMA_DICT_SIZE_MIN);

    // Round up to the next 2^n - 1 or 2^n + 2^(n - 1) - 1 depending
    // on which one is the next:
    d -= 1;
    d |= d >> 2;
    d |= d >> 3;
    d |= d >> 4;
    d |= d >> 8;
    d |= d >> 16;

    // Get the highest two bits using the proper encoding:
    out[0] = if d == u32::MAX {
        40
    } else {
        (get_dist_slot(d + 1) - 24) as u8
    };

    LzmaRet::Ok
}

pub fn lzma_lzma2_block_size(options: &mut Box<dyn std::any::Any>) -> u64 {
    let opt = options.downcast_mut::<LzmaOptionsLzma>().unwrap();

    // Use at least 1 MiB to keep compression ratio better.
    my_max((opt.dict_size as u64) * 3, 1 << 20)
}
