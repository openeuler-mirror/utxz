/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::default;

use common::my_max;

use crate::{
    api::{
        LzmaAction, LzmaBlock, LzmaCheck, LzmaFilter, LzmaRet, LzmaStream, LzmaStreamFlags,
        LZMA_BLOCK_HEADER_SIZE_MAX, LZMA_CONCATENATED, LZMA_FILTERS_MAX, LZMA_IGNORE_CHECK,
        LZMA_STREAM_HEADER_SIZE, LZMA_TELL_ANY_CHECK, LZMA_TELL_NO_CHECK,
        LZMA_TELL_UNSUPPORTED_CHECK,
    },
    check::lzma_check_is_supported,
    common::{NextCoderInitFunction, LZMA_SUPPORTED_FLAGS},
    lzma_block_header_size_decode,
};

use super::{
    lzma_block_decoder_init, lzma_block_header_decode, lzma_block_unpadded_size, lzma_bufcpy,
    lzma_end, lzma_filters_free, lzma_index_hash_append, lzma_index_hash_decode,
    lzma_index_hash_end, lzma_index_hash_init, lzma_index_hash_size, lzma_next_end,
    lzma_raw_decoder_memusage, lzma_stream_flags_compare, lzma_stream_footer_decode,
    lzma_stream_header_decode, lzma_strm_init, CoderType, LzmaIndexHash, LzmaNextCoder,
    INDEX_INDICATOR, LZMA_MEMUSAGE_BASE,
};

/// LZMA 流解码器结构体
#[derive(Debug)]
pub struct LzmaStreamDecoder {
    /// 解码序列状态，表示当前解码器所处的阶段
    sequence: Sequence,

    /// 块解码器，用于实际的数据解压缩
    block_decoder: Box<LzmaNextCoder>,

    /// 块选项，由块头部解码器解码，供块解码器使用
    block_options: LzmaBlock,

    /// 从流头部获取的流标志
    stream_flags: LzmaStreamFlags,

    /// 索引哈希，用于以 O(1) 的内存使用量比较块的大小
    /// 使用 Option 来表示可能为空的指针
    index_hash: Option<Box<LzmaIndexHash>>,

    /// 内存使用限制（以字节为单位）
    memlimit: u64,

    /// 实际需要的内存估计值（以字节为单位）
    memusage: u64,

    /// 如果为 true，当流没有完整性检查时返回 LZMA_NO_CHECK
    tell_no_check: bool,

    /// 如果为 true，当流使用了当前 liblzma 版本不支持的完整性检查时
    /// 返回 LZMA_UNSUPPORTED_CHECK
    tell_unsupported_check: bool,

    /// 如果为 true，在解码流头部后返回 LZMA_GET_CHECK
    tell_any_check: bool,

    /// 如果为 true，将告诉块解码器跳过计算和验证完整性检查
    ignore_check: bool,

    /// 如果为 true，将解码可能在中间或结尾包含流填充的连接流
    /// 当应用程序不再提供新输入（LZMA_FINISH），且我们不在流的中间，
    /// 且可能的流填充是 4 字节的倍数时，返回 StreamEnd
    concatenated: bool,

    /// 在解码连接流时，只要我们正在解码第一个流，此值为 true
    /// 这是为了避免在后续流没有有效魔数时产生误导性的 FormatError
    first_stream: bool,

    /// buffer 中的写入位置和流填充中的位置
    pos: usize,

    /// 用于保存流头部、块头部和流尾部的缓冲区
    /// 块头部具有最大的最大尺寸
    buffer: [u8; LZMA_BLOCK_HEADER_SIZE_MAX as usize],
}

impl Default for LzmaStreamDecoder {
    fn default() -> Self {
        LzmaStreamDecoder {
            sequence: Sequence::default(), // 假设 Sequence 实现了 Default
            block_decoder: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            block_options: LzmaBlock::default(), // 假设 LzmaBlock 实现了 Default
            stream_flags: LzmaStreamFlags::default(), // 假设 LzmaStreamFlags 实现了 Default
            index_hash: None,              // Option 类型默认是 None
            memlimit: 0,
            memusage: 0,
            tell_no_check: false,
            tell_unsupported_check: false,
            tell_any_check: false,
            ignore_check: false,
            concatenated: false,
            first_stream: false,
            pos: 0,
            buffer: [0; LZMA_BLOCK_HEADER_SIZE_MAX as usize], // 初始化为全零数组
        }
    }
}

/// 解码序列的枚举，表示解码过程中的不同阶段
#[derive(Debug, Default, Copy, Clone)]
pub enum Sequence {
    /// 解码流头部
    #[default]
    SeqStreamHeader,
    /// 解码块头部
    SeqBlockHeader,
    /// 初始化块
    SeqBlockInit,
    /// 运行块解码
    SeqBlockRun,
    /// 处理索引
    SeqIndex,
    /// 解码流尾部
    SeqStreamFooter,
    /// 处理流填充
    SeqStreamPadding,
}

fn stream_decoder_reset(coder: &mut LzmaStreamDecoder) -> LzmaRet {
    // 初始化用于验证索引的哈希值
    let old_index_hash = coder.index_hash.clone();
    coder.index_hash = Some(lzma_index_hash_init(old_index_hash));
    // 重置其余变量
    coder.sequence = Sequence::SeqStreamHeader;
    coder.pos = 0;

    LzmaRet::Ok
}

fn stream_decode(
    coder_ptr: &mut CoderType,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    //  println!("input: {:?}", input);
    //  println!("input length: {}", input.len());
    //  println!("input first 10 bytes: {:?}", &input[..input.len().min(10)]);

    let coder = match coder_ptr {
        CoderType::StreamDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    loop {
        match coder.sequence {
            Sequence::SeqStreamHeader => {
                // 将流头复制到内部缓冲区
                lzma_bufcpy(
                    input,
                    in_pos,
                    in_size,
                    &mut coder.buffer,
                    &mut coder.pos,
                    LZMA_STREAM_HEADER_SIZE,
                );

                // 如果还没有获取完整的流头，则返回
                if coder.pos < LZMA_STREAM_HEADER_SIZE {
                    return LzmaRet::Ok;
                }

                coder.pos = 0;

                // 解码流头
                let ret = lzma_stream_header_decode(&mut coder.stream_flags, &coder.buffer);
                if ret != LzmaRet::Ok {
                    return if ret == LzmaRet::FormatError && !coder.first_stream {
                        LzmaRet::DataError
                    } else {
                        ret
                    };
                }

                coder.first_stream = false;
                coder.block_options.check = coder.stream_flags.check.clone();
                coder.sequence = Sequence::SeqBlockHeader;

                if coder.tell_no_check && coder.stream_flags.check == LzmaCheck::None {
                    return LzmaRet::NoCheck;
                }

                if coder.tell_unsupported_check
                    && !lzma_check_is_supported(coder.stream_flags.check.clone())
                {
                    return LzmaRet::UnsupportedCheck;
                }

                if coder.tell_any_check {
                    return LzmaRet::GetCheck;
                }
                coder.sequence = Sequence::SeqBlockHeader;
                continue;
            }

            Sequence::SeqBlockHeader => {
                if *in_pos >= in_size {
                    return LzmaRet::Ok;
                }

                if coder.pos == 0 {
                    if input[*in_pos] == INDEX_INDICATOR {
                        coder.sequence = Sequence::SeqIndex;
                        continue;
                    }

                    coder.block_options.header_size =
                        lzma_block_header_size_decode!(input[*in_pos]);
                }

                lzma_bufcpy(
                    input,
                    in_pos,
                    in_size,
                    &mut coder.buffer,
                    &mut coder.pos,
                    coder.block_options.header_size as usize,
                );

                if coder.pos < coder.block_options.header_size as usize {
                    return LzmaRet::Ok;
                }

                coder.pos = 0;
                coder.sequence = Sequence::SeqBlockInit;
                continue;
            }

            Sequence::SeqBlockInit => {
                coder.block_options.version = 1;
                let mut filters: [LzmaFilter; LZMA_FILTERS_MAX + 1] = Default::default();
                coder.block_options.filters = filters.to_vec();

                let ret = lzma_block_header_decode(&mut coder.block_options, &mut coder.buffer);
                if ret != LzmaRet::Ok {
                    return ret;
                }

                coder.block_options.ignore_check = coder.ignore_check;

                let memusage = lzma_raw_decoder_memusage(&coder.block_options.filters);
                let mut ret = LzmaRet::Ok;
                if memusage == u64::MAX {
                    ret = LzmaRet::OptionsError;
                } else {
                    coder.memusage = memusage;
                    if memusage > coder.memlimit {
                        ret = LzmaRet::MemlimitError;
                    } else {
                        ret = lzma_block_decoder_init(
                            &mut coder.block_decoder,
                            &mut coder.block_options,
                        )
                    }
                };

                // 更新 filters 数组以匹配 block_options.filters 的内容
                // for (i, filter) in coder.block_options.filters.iter().enumerate() {
                //     if i < LZMA_FILTERS_MAX + 1 {
                //         filters[i] = filter.clone();
                //     }
                // }
                lzma_filters_free(&mut coder.block_options.filters);
                coder.block_options.filters = Vec::new();

                if ret != LzmaRet::Ok {
                    return ret;
                }

                coder.sequence = Sequence::SeqBlockRun;
                continue;
            }

            Sequence::SeqBlockRun => {
                let mut ret = LzmaRet::Ok;
                if let Some(code) = coder.block_decoder.code {
                    ret = code(
                        coder.block_decoder.coder.as_mut().unwrap(),
                        input,
                        in_pos,
                        in_size,
                        output,
                        out_pos,
                        out_size,
                        action.clone(),
                    );
                    if ret != LzmaRet::StreamEnd {
                        return ret;
                    }
                }

                // 从块解码器中获取实际的大小并更新 block_options
                if let Some(CoderType::BlockDecoder(block_coder)) =
                    coder.block_decoder.coder.as_mut()
                {
                    if let Some(block) = block_coder.get_block_info() {
                        coder.block_options = block;
                    }
                }

                let block_unpadded_size = lzma_block_unpadded_size(&coder.block_options);

                let ret = lzma_index_hash_append(
                    coder.index_hash.as_mut().unwrap(),
                    block_unpadded_size,
                    coder.block_options.uncompressed_size,
                );

                if ret != LzmaRet::Ok {
                    return ret;
                }

                coder.sequence = Sequence::SeqBlockHeader;
                continue;
            }

            Sequence::SeqIndex => {
                if *in_pos >= in_size {
                    return LzmaRet::Ok;
                }

                let ret = lzma_index_hash_decode(
                    coder.index_hash.as_mut().unwrap(),
                    input,
                    in_pos,
                    in_size,
                );
                if ret != LzmaRet::StreamEnd {
                    return ret;
                }

                coder.sequence = Sequence::SeqStreamFooter;
                continue;
            }

            Sequence::SeqStreamFooter => {
                lzma_bufcpy(
                    input,
                    in_pos,
                    in_size,
                    &mut coder.buffer,
                    &mut coder.pos,
                    LZMA_STREAM_HEADER_SIZE,
                );

                if coder.pos < LZMA_STREAM_HEADER_SIZE {
                    return LzmaRet::Ok;
                }

                coder.pos = 0;

                let mut footer_flags = LzmaStreamFlags::default();
                let ret = lzma_stream_footer_decode(&mut footer_flags, &coder.buffer);
                if ret != LzmaRet::Ok {
                    return if ret == LzmaRet::FormatError {
                        LzmaRet::DataError
                    } else {
                        ret
                    };
                }

                if lzma_index_hash_size(coder.index_hash.as_mut().unwrap())
                    != footer_flags.backward_size
                {
                    return LzmaRet::DataError;
                }

                let ret = lzma_stream_flags_compare(&coder.stream_flags, &footer_flags);
                if ret != LzmaRet::Ok {
                    return ret;
                }

                if !coder.concatenated {
                    return LzmaRet::StreamEnd;
                }

                coder.sequence = Sequence::SeqStreamPadding;
                continue;
            }

            Sequence::SeqStreamPadding => {
                assert!(coder.concatenated);

                loop {
                    if *in_pos >= in_size {
                        if action != LzmaAction::Finish {
                            return LzmaRet::Ok;
                        }

                        return if coder.pos == 0 {
                            LzmaRet::StreamEnd
                        } else {
                            LzmaRet::DataError
                        };
                    }

                    if input[*in_pos] != 0x00 {
                        break;
                    }

                    *in_pos += 1;
                    coder.pos = (coder.pos + 1) & 3;
                }

                if coder.pos != 0 {
                    *in_pos += 1;
                    return LzmaRet::DataError;
                }

                let ret = stream_decoder_reset(coder);
                if ret != LzmaRet::Ok {
                    return ret;
                }
                break;
            }

            _ => {
                assert!(false);
                return LzmaRet::ProgError;
            }
        }
    }
    LzmaRet::Ok
}

/// 结束流解码器并释放资源
fn stream_decoder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::StreamDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.block_decoder);
    lzma_index_hash_end(&mut coder.index_hash.as_mut().unwrap());
}

/// 获取流解码器的校验值
fn stream_decoder_get_check(coder_ptr: &mut CoderType) -> LzmaCheck {
    let coder = match coder_ptr {
        CoderType::StreamDecoder(ref mut c) => c,
        _ => return LzmaCheck::None, //，则返回错误
    };
    coder.stream_flags.check.clone()
}

/// 配置流解码器的内存使用
fn stream_decoder_memconfig(
    coder_ptr: &mut CoderType,
    memusage: &mut u64,
    old_memlimit: &mut u64,
    new_memlimit: u64,
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::StreamDecoder(ref mut c) => c,
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

pub fn lzma_stream_decoder_init(next: &mut LzmaNextCoder, memlimit: u64, flags: u32) -> LzmaRet {
    // lzma_next_coder_init!(lzma_stream_decoder_init, next, allocator);
    if next.init
        != Some(NextCoderInitFunction::StreamDecoder(
            lzma_stream_decoder_init,
        ))
    {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::StreamDecoder(
        lzma_stream_decoder_init,
    ));

    if flags & !LZMA_SUPPORTED_FLAGS != 0 {
        return LzmaRet::OptionsError;
    }

    if next.coder.is_none() {
        let coder = LzmaStreamDecoder::default();
        next.coder = Some(CoderType::StreamDecoder(coder));
        next.code = Some(stream_decode);
        next.end = Some(stream_decoder_end);
        next.get_check = Some(stream_decoder_get_check);
        next.memconfig = Some(stream_decoder_memconfig);
    }

    let coder = match &mut next.coder {
        Some(CoderType::StreamDecoder(c)) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.memlimit = my_max(1, memlimit);
    coder.memusage = LZMA_MEMUSAGE_BASE;
    coder.tell_no_check = (flags & LZMA_TELL_NO_CHECK) != 0;
    coder.tell_unsupported_check = (flags & LZMA_TELL_UNSUPPORTED_CHECK) != 0;
    coder.tell_any_check = (flags & LZMA_TELL_ANY_CHECK) != 0;
    coder.ignore_check = (flags & LZMA_IGNORE_CHECK) != 0;
    coder.concatenated = (flags & LZMA_CONCATENATED) != 0;
    coder.first_stream = true;

    stream_decoder_reset(coder)
}

pub fn lzma_stream_decoder(strm: &mut LzmaStream, memlimit: u64, flags: u32) -> LzmaRet {
    // lzma_next_strm_init(lzma_stream_decoder_init, strm, memlimit, flags);
    let ret: LzmaRet = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 避免借用冲突的初始化
    let init_ret = match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                if let Some(ref mut next) = internal.next {
                    lzma_stream_decoder_init(next, memlimit, flags)
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
                internal.supported_actions[LzmaAction::Run as usize] = true;
                internal.supported_actions[LzmaAction::Finish as usize] = true;
            }
        }
        Err(_) => return LzmaRet::ProgError,
    }

    LzmaRet::Ok
}
