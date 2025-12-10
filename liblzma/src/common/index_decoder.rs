/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_max;

use crate::{
    api::{LzmaAction, LzmaRet, LzmaStream, LzmaVli},
    check::lzma_crc32,
    common::NextCoderInitFunction,
};

use super::{
    lzma_end, lzma_index_append, lzma_index_end, lzma_index_init, lzma_index_memusage,
    lzma_index_padding_size, lzma_index_prealloc, lzma_next_end, lzma_strm_init, lzma_vli_decode,
    CoderType, LzmaIndex, LzmaNextCoder, INDEX_INDICATOR, UNPADDED_SIZE_MAX, UNPADDED_SIZE_MIN,
};

use std::sync::{Arc, Mutex};

/// 用于表示 LZMA 索引解码器的结构体
#[derive(Debug)]
pub struct LzmaIndexDecoder {
    /// 解码序列状态
    pub sequence: Sequence,

    /// 内存使用限制
    pub memlimit: u64,

    /// 目标索引
    pub index: Option<Box<LzmaIndex>>,

    /// 应用程序提供的指针，在成功解码后设置
    pub index_ptr: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,

    /// 剩余待解码的记录数
    pub count: LzmaVli,

    /// 最近的未填充大小字段
    pub unpadded_size: LzmaVli,

    /// 最近的未压缩大小字段
    pub uncompressed_size: LzmaVli,

    /// 整数中的位置
    pub pos: usize,

    /// 记录列表字段的 CRC32
    pub crc32: u32,
}

impl Clone for LzmaIndexDecoder {
    fn clone(&self) -> Self {
        LzmaIndexDecoder {
            sequence: self.sequence,
            memlimit: self.memlimit,
            // 安全方式：克隆时将 mutable 引用置空
            index: None,
            index_ptr: None,
            count: self.count,
            unpadded_size: self.unpadded_size,
            uncompressed_size: self.uncompressed_size,
            pos: self.pos,
            crc32: self.crc32,
        }
    }
}

impl Default for LzmaIndexDecoder {
    fn default() -> Self {
        LzmaIndexDecoder {
            sequence: Sequence::Indicator, // 假设 `Sequence` 实现了 `Default` 或者默认是 `SeqInit`
            memlimit: 0,
            index: None,          // `Option<&'a mut LzmaIndex>` 默认是 None
            index_ptr: None,      // `Option<&'a mut LzmaIndex>` 默认是 None
            count: 0,             // 假设 `LzmaVli` 是某种类型，可以初始化为 0
            unpadded_size: 0,     // 假设 `LzmaVli` 可以初始化为 0
            uncompressed_size: 0, // 假设 `LzmaVli` 可以初始化为 0
            pos: 0,
            crc32: 0,
        }
    }
}
/// 解码序列的枚举，表示解码过程中的不同阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sequence {
    Indicator,
    Count,
    MemUsage,
    Unpadded,
    Uncompressed,
    PaddingInit,
    Padding,
    Crc32,
}

fn index_decode(
    coder_ptr: &mut CoderType,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_mut::<LzmaIndexDecoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::IndexDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    let in_start = *in_pos;
    let mut ret = LzmaRet::Ok;

    while *in_pos < in_size {
        match coder.sequence {
            Sequence::Indicator => {
                if input[*in_pos] != INDEX_INDICATOR {
                    return LzmaRet::DataError;
                }
                *in_pos += 1;
                coder.sequence = Sequence::Count;
            }
            Sequence::Count => {
                ret = lzma_vli_decode(
                    &mut coder.count,
                    Some(&mut coder.pos),
                    input,
                    in_pos,
                    in_size,
                );
                if ret != LzmaRet::StreamEnd {
                    break;
                }
                coder.pos = 0;
                coder.sequence = Sequence::MemUsage;
            }
            Sequence::MemUsage => {
                if lzma_index_memusage(1, coder.count) > coder.memlimit {
                    ret = LzmaRet::MemlimitError;
                    break;
                }
                // Convert Box<LzmaIndex> to Arc<Mutex<LzmaIndex>> for prealloc, then update the Box
                if let Some(index_box) = coder.index.as_mut() {
                    let arc = Arc::new(Mutex::new((**index_box).clone()));
                    lzma_index_prealloc(arc.clone(), coder.count);
                    **index_box = arc.lock().unwrap().clone();
                }
                coder.sequence = if coder.count == 0 {
                    Sequence::PaddingInit
                } else {
                    Sequence::Unpadded
                };
            }
            Sequence::Unpadded | Sequence::Uncompressed => {
                let size = if coder.sequence == Sequence::Unpadded {
                    &mut coder.unpadded_size
                } else {
                    &mut coder.uncompressed_size
                };
                ret = lzma_vli_decode(size, Some(&mut coder.pos), input, in_pos, in_size);
                if ret != LzmaRet::StreamEnd {
                    break;
                }
                coder.pos = 0;
                if coder.sequence == Sequence::Unpadded {
                    if coder.unpadded_size < UNPADDED_SIZE_MIN
                        || coder.unpadded_size > UNPADDED_SIZE_MAX
                    {
                        return LzmaRet::DataError;
                    }
                    coder.sequence = Sequence::Uncompressed;
                } else {
                    let temp = lzma_index_append(
                        &mut coder.index.as_mut().unwrap(),
                        coder.unpadded_size,
                        coder.uncompressed_size,
                    );
                    if temp != LzmaRet::Ok {
                        return temp;
                    }
                    coder.count -= 1;
                    coder.sequence = if coder.count == 0 {
                        Sequence::PaddingInit
                    } else {
                        Sequence::Unpadded
                    };
                }
            }
            Sequence::PaddingInit => {
                coder.pos = lzma_index_padding_size(&coder.index.as_mut().unwrap()) as usize;
                coder.sequence = Sequence::Padding;
            }
            Sequence::Padding => {
                if coder.pos > 0 {
                    coder.pos -= 1;
                    if input[*in_pos] != 0x00 {
                        return LzmaRet::DataError;
                    }
                    *in_pos += 1;
                } else {
                    coder.crc32 = lzma_crc32(
                        &input[in_start..*in_pos],
                        *in_pos - in_start as usize,
                        coder.crc32,
                    );
                    coder.sequence = Sequence::Crc32;
                }
            }
            Sequence::Crc32 => {
                while coder.pos < 4 {
                    if *in_pos == in_size {
                        return LzmaRet::Ok;
                    }
                    if ((coder.crc32 >> (coder.pos * 8)) & 0xFF) != input[*in_pos].into() {
                        return LzmaRet::DataError;
                    }
                    *in_pos += 1;
                    coder.pos += 1;
                }

                // *coder->index_ptr = coder->index;
                // 将解码后的索引赋值给应用程序提供的指针
                match coder.index_ptr {
                    Some(ref mut index_ptr) => {
                        *index_ptr = Arc::new(Mutex::new(Arc::new(Mutex::new(
                            *coder.index.take().unwrap(),
                        ))));
                    }
                    None => {
                        // 如果指针为空，则创建新的指针并赋值
                        coder.index_ptr = Some(Arc::new(Mutex::new(Arc::new(Mutex::new(
                            *coder.index.take().unwrap(),
                        )))));
                    }
                }

                return LzmaRet::StreamEnd;
            }
            _ => {
                return LzmaRet::ProgError;
            }
        }
    }

    let in_used = *in_pos - in_start;
    if in_used > 0 {
        coder.crc32 = lzma_crc32(&input[in_start..*in_pos], in_used as usize, coder.crc32);
    }

    ret
}

/// 结束解码器的操作
/// 释放相关资源
fn index_decoder_end(coder_ptr: &mut CoderType) {
    // 释放索引相关的资源
    // let mut coder = coder_ptr.unwrap().downcast_mut::<LzmaIndexDecoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::IndexDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };

    if coder.index.is_some() {
        lzma_index_end(coder.index.as_mut().unwrap());
    }
}

/// 配置内存限制
/// 更新内存使用量和内存限制
fn index_decoder_memconfig(
    coder_ptr: &mut CoderType,
    memusage: &mut u64,
    old_memlimit: &mut u64,
    new_memlimit: u64,
) -> LzmaRet {
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::IndexDecoder(ref mut c) => c,
        _ => return LzmaRet::MemError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // 获取当前内存使用量
    *memusage = lzma_index_memusage(1, coder.count);

    // 保存当前的内存限制
    *old_memlimit = coder.memlimit;

    // 如果提供了新的内存限制，检查其是否合法
    if new_memlimit != 0 {
        if new_memlimit < *memusage {
            return LzmaRet::MemlimitError; // 如果新的内存限制小于当前内存使用量，返回错误
        }

        // 更新内存限制
        coder.memlimit = new_memlimit;
    }

    // 返回成功状态
    LzmaRet::Ok
}

/// 重置解码器的状态
/// 重新初始化解码器，并为索引分配内存
fn index_decoder_reset(
    coder: &mut LzmaIndexDecoder,
    mut i: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    memlimit: u64,
) -> LzmaRet {
    // 记住传入的指针（给定的应用指针）。只有在解码成功后，
    // 我们才会将它指向已解码的 Index。
    // 在此之前，保持它为 None，以便应用程序可以始终安全地
    // 将它传递给 lzma_index_end()，无论解码是否成功。
    coder.index_ptr = i;
    i = None;

    // 总是分配一个新的 lzma_index。
    coder.index = lzma_index_init().map(|arc| Box::new(arc.lock().unwrap().clone()));

    if coder.index.is_none() {
        return LzmaRet::MemError;
    }

    // 初始化其余字段
    coder.sequence = Sequence::Indicator;
    coder.memlimit = my_max(1, memlimit);
    coder.count = 0; // 需要初始化，因为在 _memconfig() 中使用到
    coder.pos = 0;
    coder.crc32 = 0;

    LzmaRet::Ok
}

/// 初始化索引解码器
pub fn lzma_index_decoder_init(
    next: &mut LzmaNextCoder,
    i: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    memlimit: u64,
) -> LzmaRet {
    // 初始化下一个解码器
    // lzma_next_coder_init!(&lzma_index_decoder_init, next, allocator);
    if next.init != Some(NextCoderInitFunction::IndexDecoder(lzma_index_decoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::IndexDecoder(lzma_index_decoder_init));

    // if i.is_none() {
    //     return LzmaRet::ProgError;
    // }

    let mut coder: &mut LzmaIndexDecoder = &mut LzmaIndexDecoder::default();
    if next.coder.is_none() {
        let coder_ = LzmaIndexDecoder::default();
        next.code = Some(index_decode);
        next.end = Some(index_decoder_end);
        next.memconfig = Some(index_decoder_memconfig);
        next.coder = Some(CoderType::IndexDecoder(coder_));
    } else {
        lzma_index_end(coder.index.as_mut().unwrap());
    }
    coder = match next.coder {
        Some(CoderType::IndexDecoder(ref mut c)) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.index = None;

    index_decoder_reset(coder, i, memlimit)
}

const LZMA_RUN: usize = 0;
const LZMA_FINISH: usize = 3;
pub fn lzma_index_decoder(
    strm: &mut LzmaStream,
    i: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    memlimit: u64,
) -> LzmaRet {
    // 初始化流解码器
    // 初始化流
    let ret = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 避免借用冲突的初始化
    let init_ret = match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                if let Some(ref mut next) = internal.next {
                    lzma_index_decoder_init(next, i, memlimit)
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
                internal.supported_actions[LZMA_RUN] = true;
                internal.supported_actions[LZMA_FINISH] = true;
            }
        }
        Err(_) => return LzmaRet::ProgError,
    }

    LzmaRet::Ok
}

pub fn lzma_index_buffer_decode(
    mut i: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    memlimit: &mut u64,
    in_data: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    // 基本检查
    if i.is_none()
        || Some(memlimit.clone()).is_none()
        || in_data.is_empty()
        || Some(in_pos.clone()).is_none()
        || *in_pos > in_size
    {
        return LzmaRet::ProgError;
    }

    // 初始化解码器
    let mut coder = LzmaIndexDecoder {
        sequence: Sequence::Indicator,
        memlimit: 0,
        count: 0,
        pos: 0,
        crc32: 0,
        index: None,
        index_ptr: None,
        uncompressed_size: 0,
        unpadded_size: 0,
    };
    let ret = index_decoder_reset(&mut coder, i, *memlimit);
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 保存输入的起始位置，以便在出错时恢复
    let in_start = *in_pos;

    // 实际解码
    let mut ret = index_decode(
        &mut CoderType::IndexDecoder(coder.clone()),
        in_data,
        in_pos,
        in_size,
        &mut Vec::new(),
        &mut 0,
        0,
        LzmaAction::Run,
    );

    if ret == LzmaRet::StreamEnd {
        ret = LzmaRet::Ok;
    } else {
        // 出错，释放索引结构并恢复输入位置

        lzma_index_end(&mut coder.index.unwrap());
        *in_pos = in_start;

        if ret == LzmaRet::Ok {
            // 输入数据被截断或损坏
            // 使用 LZMA_DATA_ERROR 而不是 LZMA_BUF_ERROR
            ret = LzmaRet::DataError;
        } else if ret == LzmaRet::MemlimitError {
            // 告诉调用者需要多少内存
            *memlimit = lzma_index_memusage(1, coder.count);
        }
    }

    ret
}
