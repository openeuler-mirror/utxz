/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::{my_max, read32le, read64le};

use crate::{
    api::{
        LzmaAction, LzmaCheck, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream,
        LZMA_CONCATENATED, LZMA_FILTER_LZMA1, LZMA_IGNORE_CHECK, LZMA_TELL_ANY_CHECK,
    },
    check::lzma_crc32,
    common::NextCoderInitFunction,
    lzma::lzma_lzma_decoder_init,
};

use super::{
    lzma_bufcpy, lzma_end, lzma_next_end, lzma_next_filter_init, lzma_strm_init, CoderType,
    LzmaFilterInfo, LzmaNextCoder, LZMA_MEMUSAGE_BASE, LZMA_SUPPORTED_FLAGS,
};

/// .lz 格式版本 0 缺少尾部中的 64 位成员大小字段
const LZIP_V0_FOOTER_SIZE: usize = 12;
const LZIP_V1_FOOTER_SIZE: usize = 20;
const LZIP_FOOTER_SIZE_MAX: usize = LZIP_V1_FOOTER_SIZE;

// lc/lp/pb 在 .lz 格式中是硬编码的
const LZIP_LC: u32 = 3;
const LZIP_LP: u32 = 0;
const LZIP_PB: u32 = 2;

/// 解码过程中的状态序列
#[derive(Debug, Clone, Copy, PartialEq)]
enum DecodingSequence {
    SeqIdString,     // 解码 ID 字符串
    SeqVersion,      // 解码版本
    SeqDictSize,     // 解码字典大小
    SeqCoderInit,    // 解码器初始化
    SeqLzmaStream,   // 解码 LZMA 流
    SeqMemberFooter, // 解码成员尾部
}

/// LZMA 解码器结构体，用于处理 .lz 格式
#[derive(Debug)]
pub struct LzmaLzipCoder {
    /// 当前解码状态
    sequence: DecodingSequence,

    /// .lz 成员格式版本
    version: u32,

    /// 解压后的数据 CRC32 校验和
    crc32: u32,

    /// 解压后的数据大小
    uncompressed_size: u64,

    /// 成员的压缩大小
    member_size: u64,

    /// 内存使用限制
    memlimit: u64,

    /// 实际需要的内存量
    memusage: u64,

    /// 如果为 true，则在解码头部字段后返回 LZMA_GET_CHECK
    tell_any_check: bool,

    /// 如果为 true，则跳过 CRC32 校验
    ignore_check: bool,

    /// 如果为 true，则解码连接的 .lz 成员，并在解码第一个成员后遇到非 .lz 数据时停止
    concatenated: bool,

    /// 在解码连接的 .lz 成员时，表示当前正在解码第一个 .lz 成员
    first_member: bool,

    /// 当前头部和尾部字段的读取位置
    pos: usize,

    /// 用于存储 .lz 文件尾部字段的缓冲区
    buffer: [u8; LZIP_FOOTER_SIZE_MAX],

    /// 从 .lz 头部解码的选项，用于初始化 LZMA1 解码器
    options: LzmaOptionsLzma,

    /// LZMA1 解码器实例
    lzma_decoder: Box<LzmaNextCoder>,
}

impl Default for LzmaLzipCoder {
    fn default() -> Self {
        Self {
            sequence: DecodingSequence::SeqIdString,
            version: 0,
            crc32: 0,
            uncompressed_size: 0,
            member_size: 0,
            memlimit: 0,
            memusage: 0,
            tell_any_check: false,
            ignore_check: false,
            concatenated: false,
            first_member: true,
            pos: 0,
            buffer: [0; LZIP_FOOTER_SIZE_MAX],
            options: LzmaOptionsLzma::default(),
            lzma_decoder: Box::new(LzmaNextCoder::default()),
        }
    }
}

// 主解码函数
pub fn lzip_decode(
    coder_ptr: &mut CoderType,

    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let mut coder = coder_ptr.downcast_mut::<LzmaLzipCoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::LzipDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    loop {
        match coder.sequence {
            DecodingSequence::SeqIdString => {
                // LZIP 魔数是 ASCII 的 "LZIP"
                let lzip_id_string = [0x4C, 0x5A, 0x49, 0x50];

                while coder.pos < lzip_id_string.len() {
                    if *in_pos >= input.len() {
                        // 如果是第二个或之后的连接成员且输入结束
                        // 在读取魔数之前，丢弃已读取的字节并结束
                        return if !coder.first_member && action.clone() == LzmaAction::Finish {
                            LzmaRet::StreamEnd
                        } else {
                            LzmaRet::Ok
                        };
                    }

                    if input[*in_pos] != lzip_id_string[coder.pos] {
                        // .lz 格式允许在文件末尾放置非 .lz 数据
                        // 如果我们已经看到至少一个有效的 .lz 成员
                        // 则不消费 *in_pos 处的字节并返回 STREAM_END
                        return if !coder.first_member {
                            LzmaRet::StreamEnd
                        } else {
                            LzmaRet::FormatError
                        };
                    }

                    *in_pos += 1;
                    coder.pos += 1;
                }

                coder.pos = 0;
                coder.crc32 = 0;
                coder.uncompressed_size = 0;
                coder.member_size = lzip_id_string.len() as u64;
                coder.sequence = DecodingSequence::SeqVersion;
            }

            DecodingSequence::SeqVersion => {
                if *in_pos >= input.len() {
                    return LzmaRet::Ok;
                }

                coder.version = input[*in_pos as usize] as u32;
                *in_pos += 1;

                // 我们支持版本 0 和未扩展的版本 1
                if coder.version > 1 {
                    return LzmaRet::OptionsError;
                }

                coder.member_size += 1;
                coder.sequence = DecodingSequence::SeqDictSize;

                // 如果应用程序想知道完整性检查类型，现在可以告诉它
                if coder.tell_any_check {
                    return LzmaRet::GetCheck;
                }
            }

            DecodingSequence::SeqDictSize => {
                if *in_pos >= input.len() {
                    return LzmaRet::Ok;
                }

                let ds = input[*in_pos];
                *in_pos += 1;
                coder.member_size += 1;

                // 最低 5 位用于字典大小的以 2 为底的对数
                // 最高 3 位是分数部分（0/16 到 7/16）
                let b2log = ds & 0x1F;
                let fracnum = ds >> 5;

                // 格式版本 0 和 1 允许字典大小在 [4 KiB, 512 MiB] 范围内
                if b2log < 12 || b2log > 29 || (b2log == 12 && fracnum > 0) {
                    return LzmaRet::DataError;
                }

                // 计算字典大小
                let dict_size = (1u32 << b2log) - (fracnum << (b2log - 4)) as u32;

                // 更新状态...
                coder.sequence = DecodingSequence::SeqCoderInit;
            }
            DecodingSequence::SeqCoderInit => {
                if coder.memusage > coder.memlimit {
                    return LzmaRet::MemlimitError;
                }

                // let mut tmp: Box<dyn std::any::Any> = Box::new(coder.options);
                let filters: [LzmaFilterInfo; 2] = [
                    LzmaFilterInfo {
                        id: LZMA_FILTER_LZMA1,
                        init: Some(lzma_lzma_decoder_init),
                        options: Some(LzmaOptionsType::LzmaOptionsLzma(coder.options.clone())),
                    },
                    LzmaFilterInfo {
                        id: 0,
                        init: None,
                        options: None,
                    },
                ];

                // 模拟调用 lzma_next_filter_init 函数
                let ret = lzma_next_filter_init(&mut coder.lzma_decoder, &filters);
                if ret != LzmaRet::Ok {
                    return ret;
                }

                coder.crc32 = 0;
                coder.sequence = DecodingSequence::SeqLzmaStream;
            }

            DecodingSequence::SeqLzmaStream => {
                let in_start = *in_pos;
                let out_start = *out_pos;

                // 模拟 LZMA 解码
                let ret: LzmaRet = LzmaRet::Ok;
                if let Some(code) = coder.lzma_decoder.code {
                    code(
                        &mut coder.lzma_decoder.coder.as_mut().unwrap(),
                        input,
                        in_pos,
                        in_size,
                        out,
                        out_pos,
                        out_size,
                        action.clone(),
                    );
                }

                let out_used = *out_pos - out_start;
                coder.member_size += (*in_pos - in_start) as u64;
                coder.uncompressed_size += out_used as u64;

                if !coder.ignore_check && out_used > 0 {
                    coder.crc32 = lzma_crc32(
                        &out[out_start..out_start + out_used],
                        out_used as usize,
                        coder.crc32,
                    );
                }

                if ret != LzmaRet::StreamEnd {
                    return ret;
                }

                coder.sequence = DecodingSequence::SeqMemberFooter;
            }

            DecodingSequence::SeqMemberFooter => {
                let footer_size = if coder.version == 0 {
                    LZIP_V0_FOOTER_SIZE
                } else {
                    LZIP_V1_FOOTER_SIZE
                };

                lzma_bufcpy(
                    input,
                    in_pos,
                    in_size,
                    &mut coder.buffer,
                    &mut coder.pos,
                    footer_size,
                );

                if coder.pos < footer_size {
                    return LzmaRet::Ok;
                }

                coder.pos = 0;
                coder.member_size += footer_size as u64;

                if !coder.ignore_check && coder.crc32 != read32le(&coder.buffer[0..4]) {
                    return LzmaRet::DataError;
                }

                if coder.uncompressed_size != read64le(&coder.buffer[4..12]) {
                    return LzmaRet::DataError;
                }

                if coder.version > 0 {
                    if coder.member_size != read64le(&coder.buffer[12..20]) {
                        return LzmaRet::DataError;
                    }
                }

                if !coder.concatenated {
                    return LzmaRet::StreamEnd;
                }

                coder.first_member = false;
                coder.sequence = DecodingSequence::SeqCoderInit;
            }

            // 其他匹配分支的实现...
            _ => {
                // 处理其他序列状态
                return LzmaRet::ProgError;
            }
        }
    }
}

fn lzip_decoder_end(mut coder_ptr: &mut CoderType) {
    // let coder = coder_ptr.unwrap().downcast_mut::<LzmaLzipCoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::LzipDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.lzma_decoder);
    // 释放内存：在 Rust 中通常使用 Box 自动管理内存
}

fn lzip_decoder_get_check(_coder_ptr: &mut CoderType) -> LzmaCheck {
    LzmaCheck::Crc32
}

fn lzip_decoder_memconfig(
    mut coder_ptr: &mut CoderType,
    memusage: &mut u64,
    old_memlimit: &mut u64,
    new_memlimit: u64,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_mut::<LzmaLzipCoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::LzipDecoder(ref mut c) => c,
        _ => return LzmaRet::MemError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    *memusage = coder.memusage;
    *old_memlimit = coder.memlimit;

    if new_memlimit != 0 {
        if new_memlimit < coder.memusage {
            return LzmaRet::MemlimitError;
        }
        coder.memlimit = new_memlimit;
    }

    LzmaRet::Ok
}

fn lzma_lzip_decoder_init(next: &mut LzmaNextCoder, memlimit: u64, flags: u32) -> LzmaRet {
    // lzma_next_coder_init(&lzma_lzip_decoder_init, next, allocator);
    if next.init != Some(NextCoderInitFunction::LzipDecoder(lzma_lzip_decoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::LzipDecoder(lzma_lzip_decoder_init));

    // 初始化 lzma_next_coder
    if flags & !LZMA_SUPPORTED_FLAGS != 0 {
        return LzmaRet::OptionsError;
    }

    let mut coder: &mut LzmaLzipCoder = &mut LzmaLzipCoder::default();

    if next.coder.is_none() {
        let coder_ = LzmaLzipCoder::default();
        next.code = Some(lzip_decode); // 假设 lzip_decode 是解码函数
        next.end = Some(lzip_decoder_end);
        next.get_check = Some(lzip_decoder_get_check);
        next.memconfig = Some(lzip_decoder_memconfig);
        next.coder = Some(CoderType::LzipDecoder(coder_));
        coder.lzma_decoder = Box::new(LzmaNextCoder::default());
    } else {
        coder = match next.coder {
            Some(CoderType::LzipDecoder(ref mut c)) => c,
            _ => return LzmaRet::ProgError,
        };
    }

    coder.sequence = DecodingSequence::SeqIdString;
    coder.memlimit = my_max(1, memlimit);
    coder.memusage = LZMA_MEMUSAGE_BASE;
    coder.tell_any_check = flags & LZMA_TELL_ANY_CHECK != 0;
    coder.ignore_check = flags & LZMA_IGNORE_CHECK != 0;
    coder.concatenated = flags & LZMA_CONCATENATED != 0;
    coder.first_member = true;
    coder.pos = 0;

    LzmaRet::Ok
}

pub fn lzma_lzip_decoder(strm: &mut LzmaStream, memlimit: u64, flags: u32) -> LzmaRet {
    // lzma_next_strm_init(lzma_lzip_decoder_init, strm, memlimit, flags);
    let ret_: LzmaRet = lzma_strm_init(Some(strm));
    if ret_ != LzmaRet::Ok {
        return ret_;
    }

    // 避免借用冲突的初始化
    let init_ret = match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                if let Some(ref mut next) = internal.next {
                    lzma_lzip_decoder_init(next, memlimit, flags)
                } else {
                    LzmaRet::ProgError
                }
            } else {
                LzmaRet::ProgError
            }
        }
        Err(_) => LzmaRet::ProgError,
    };

    if init_ret != LzmaRet::Ok {
        lzma_end(Some(strm));
        return init_ret;
    }

    // 设置支持的操作
    match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                internal.supported_actions[0] = true; // LZMA_RUN
                internal.supported_actions[1] = true; // LZMA_FINISH
            }
        }
        Err(_) => return LzmaRet::ProgError,
    }

    LzmaRet::Ok
}
