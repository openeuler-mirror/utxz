/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::ptr;

use common::my_min;

use crate::{
    api::{LzmaAction, LzmaOptionsType, LzmaRet, LzmaVli},
    common::{
        lzma_bufcpy, lzma_next_end, lzma_next_filter_init, CoderType, LzmaFilterInfo,
        LzmaNextCoder, LZMA_BUFFER_SIZE,
    },
    lzma::{self, LzmaLzma1Decoder, LzmaLzma2Decoder, LzmaLzma2Encoder},
};

#[derive(Debug, Clone)]
pub struct LzmaDict {
    pub buf: Vec<u8>,
    pub pos: usize,
    pub full: usize,
    pub limit: usize,
    pub size: usize,
    pub need_reset: bool,
}
impl Default for LzmaDict {
    fn default() -> Self {
        // 使用一个空的 Vec<u8> 来初始化 buf
        LzmaDict {
            buf: Vec::new(), // 初始化为一个空 Vec
            pos: 0,
            full: 0,
            limit: 0,
            size: 0,
            need_reset: false,
        }
    }
}
pub struct LzmaLzDecoderOptions {
    pub dict_size: usize,
    pub preset_dict: Vec<u8>,
    pub preset_dict_size: usize,
}

// pub const LZMA_LZ_DECODER_INIT: LzmaLzDecoder = LzmaLzDecoder {
//     coder: &mut Box::new(Box::new(()) as Box<dyn std::any::Any>),
//     code: None,
//     reset: None,
//     set_uncompressed: None,
//     end: None,
// };

#[derive(Debug)]
pub enum LzCoderType {
    Lzma2Decoder(LzmaLzma2Decoder),
    LzmaDecoder(LzmaLzma1Decoder),
}

#[derive(Debug)]
pub struct LzmaLzDecoder {
    pub coder: Option<LzCoderType>,
    pub code: Option<
        fn(
            coder: &mut LzCoderType,
            dict: &mut LzmaDict,
            in_: &[u8],
            in_pos: &mut usize,
            in_size: usize,
        ) -> LzmaRet,
    >,
    pub reset: Option<fn(coder: &mut LzCoderType, options: &LzmaOptionsType)>,
    pub set_uncompressed:
        Option<fn(coder: &mut LzCoderType, uncompressed_size: LzmaVli, allow_eopm: bool)>,
    pub end: Option<fn(coder: &mut LzCoderType)>,
}

impl Default for LzmaLzDecoder {
    fn default() -> Self {
        LzmaLzDecoder {
            coder: None,            // 假设 LzCoderType 实现了 Default
            code: None,             // Option 类型默认是 None
            reset: None,            // Option 类型默认是 None
            set_uncompressed: None, // Option 类型默认是 None
            end: None,              // Option 类型默认是 None
        }
    }
}

#[derive(Debug)]
pub struct LzmaDecoder {
    dict: LzmaDict,
    lz: LzmaLzDecoder,
    next: Box<LzmaNextCoder>,
    next_finished: bool,
    this_finished: bool,
    temp: TempBuffer,
}
impl Default for LzmaDecoder {
    fn default() -> Self {
        LzmaDecoder {
            dict: LzmaDict::default(),                // 假设 LzmaDict 实现了 Default
            lz: LzmaLzDecoder::default(),             // 假设 LzmaLzDecoder 实现了 Default
            next: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            next_finished: false,
            this_finished: false,
            temp: TempBuffer::default(), // 假设 TempBuffer 实现了 Default
        }
    }
}

#[derive(Debug)]
pub struct TempBuffer {
    pub pos: usize,
    pub size: usize,
    pub buffer: [u8; LZMA_BUFFER_SIZE],
}

impl Default for TempBuffer {
    fn default() -> Self {
        TempBuffer {
            pos: 0,
            size: 0,
            buffer: [0; LZMA_BUFFER_SIZE], // 初始化为全零数组
        }
    }
}
//////////////////////
// Inline functions //
//////////////////////
#[inline]
pub fn dict_get(dict: &LzmaDict, distance: u32) -> u8 {
    let temp = if (distance as usize) < (dict.pos) {
        0
    } else {
        dict.size
    };
    let count = dict
        .pos
        .wrapping_sub(distance as usize)
        .wrapping_sub(1)
        .wrapping_add(temp);
    dict.buf[count]
}

/// Unsafe variant of dict_get without Vec bounds checking.
/// Caller must ensure the computed index is within buf bounds
/// (i.e., the distance is valid and the arithmetic doesn't overflow).
#[inline]
pub unsafe fn dict_get_unchecked(dict: &LzmaDict, distance: u32) -> u8 {
    let temp = if (distance as usize) < dict.pos {
        0
    } else {
        dict.size
    };
    let count = dict
        .pos
        .wrapping_sub(distance as usize)
        .wrapping_sub(1)
        .wrapping_add(temp);
    *dict.buf.as_ptr().add(count)
}

#[inline]
pub fn dict_is_empty(dict: &LzmaDict) -> bool {
    dict.full == 0
}

#[inline]
pub fn dict_is_distance_valid(dict: &LzmaDict, distance: usize) -> bool {
    dict.full > distance
}

#[inline]
pub fn dict_repeat(dict: &mut LzmaDict, distance: usize, len: &mut usize) -> bool {
    // 确保不会写入超过字典限制
    let dict_avail = dict.limit - dict.pos;
    let mut left = my_min(dict_avail, *len);
    *len -= left;

    // 根据不同情况处理数据复制
    if distance < left {
        if distance < dict.pos {
            // 源和目标区域重叠，但未发生环绕。
            // 使用 unsafe ptr::copy 消除边界检查。
            let src = dict.buf.as_ptr();
            let dst = dict.buf.as_mut_ptr();
            let start = dict.pos - distance - 1;
            // 前 distance 个字节：源 (start..start+distance) 与目标 (dict.pos..dict.pos+distance) 不重叠
            unsafe {
                ptr::copy_nonoverlapping(src.add(start), dst.add(dict.pos), distance);
            }
            dict.pos += distance;
            left -= distance;
            // 剩余部分可能重叠，使用 ptr::copy (memmove 语义)
            unsafe {
                ptr::copy(src.add(start), dst.add(dict.pos), left);
            }
            dict.pos += left;
        } else {
            // 环绕情况下的重叠复制，逐字节复制
            while left > 0 {
                dict.buf[dict.pos] = dict_get(dict, distance as u32);
                dict.pos += 1;
                left -= 1;
            }
        }
    } else if distance < dict.pos {
        // 不重叠，直接块复制
        let start = dict.pos - distance - 1;
        unsafe {
            ptr::copy_nonoverlapping(
                dict.buf.as_ptr().add(start),
                dict.buf.as_mut_ptr().add(dict.pos),
                left,
            );
        }
        dict.pos += left;
    } else {
        // 字典需要"环绕"，可能需要两次复制
        assert_eq!(dict.full, dict.size);
        let copy_pos = dict.pos + dict.size - distance - 1;
        let mut copy_size = dict.size - copy_pos;

        if copy_size < left {
            // 第一次复制
            dict.buf
                .copy_within(copy_pos..copy_pos + copy_size, dict.pos);
            dict.pos += copy_size;

            // 第二次复制
            copy_size = left - copy_size;
            dict.buf.copy_within(0..copy_size, dict.pos);
            dict.pos += copy_size;
        } else {
            // 单次复制即可
            dict.buf.copy_within(copy_pos..copy_pos + left, dict.pos);
            dict.pos += left;
        }
    }

    // 更新字典的填充状态
    if dict.full < dict.pos {
        dict.full = dict.pos;
    }

    // 如果 len 不为 0，返回 true 表示还有剩余数据未处理
    *len != 0
}

#[inline]
pub fn dict_put(dict: &mut LzmaDict, byte: u8) -> bool {
    // 如果字典已满，则返回 true
    if dict.pos == dict.limit {
        return true;
    }

    // 将字节写入字典并更新位置
    dict.buf[dict.pos] = byte;
    dict.pos += 1;

    // 更新字典的已填充大小
    if dict.pos > dict.full {
        dict.full = dict.pos;
    }

    // 返回 false 表示成功写入
    false
}

/// Unsafe variant of dict_put without Vec bounds checking.
/// Caller must ensure dict.pos < dict.limit (dictionary is not full).
#[inline]
pub unsafe fn dict_put_unchecked(dict: &mut LzmaDict, byte: u8) {
    *dict.buf.as_mut_ptr().add(dict.pos) = byte;
    dict.pos += 1;
    if dict.pos > dict.full {
        dict.full = dict.pos;
    }
}

#[inline]
pub fn dict_write(
    dict: &mut LzmaDict,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    left: &mut usize,
) {
    // 如果输入数据的剩余大小大于剩余空间，则调整输入大小
    let in_size = if in_size - *in_pos > *left {
        *in_pos + *left
    } else {
        in_size
    };

    // 复制数据到字典缓冲区
    *left -= lzma_bufcpy(
        input,
        in_pos,
        in_size,
        &mut dict.buf,
        &mut dict.pos,
        dict.limit,
    );

    // 更新字典的已填充大小
    if dict.pos > dict.full {
        dict.full = dict.pos;
    }
}

#[inline]
pub fn dict_reset(dict: &mut LzmaDict) {
    dict.need_reset = true;
}

pub fn lz_decoder_reset(coder: &mut LzmaDecoder) {
    // 重置字典的位置和已填充大小
    coder.dict.pos = 0;
    coder.dict.full = 0;

    // 将字典缓冲区的最后一个字节设置为 '\0'
    if let Some(last) = coder.dict.buf.last_mut() {
        *last = b'\0';
    }

    // 重置字典的 need_reset 标志
    coder.dict.need_reset = false;
}

pub fn decode_buffer(
    coder: &mut LzmaDecoder,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    loop {
        // 如果需要，重置字典的位置
        if coder.dict.pos == coder.dict.size {
            coder.dict.pos = 0;
        }

        // 存储当前字典位置，用于知道从哪里开始复制到输出缓冲区
        let dict_start = coder.dict.pos;

        // 计算允许解码的最大字节数，不能超过字典缓冲区的末尾，
        // 也不能超过填满输出缓冲区所需的字节数
        coder.dict.limit =
            coder.dict.pos + my_min(out_size - *out_pos, coder.dict.size - coder.dict.pos);

        // 调用 coder.lz.code() 进行实际解码
        let mut ret = LzmaRet::Ok;
        if let Some(code) = coder.lz.code {
            ret = code(
                &mut coder.lz.coder.as_mut().unwrap(),
                &mut coder.dict,
                input,
                in_pos,
                in_size,
            );
        }

        // 将解码后的数据从字典复制到输出缓冲区
        let copy_size = coder.dict.pos - dict_start;
        assert!(copy_size <= out_size - *out_pos);

        if copy_size > 0 {
            unsafe {
                ptr::copy_nonoverlapping(
                    coder.dict.buf.as_ptr().add(dict_start),
                    output.as_mut_ptr().add(*out_pos),
                    copy_size,
                );
            }
        }

        *out_pos += copy_size;

        // 如果需要，重置字典
        if coder.dict.need_reset {
            lz_decoder_reset(coder);

            // 如果解码完成或发生错误，或者输出缓冲区已满，则返回
            if ret != LzmaRet::Ok || *out_pos == out_size {
                return ret;
            }
        } else {
            // 如果解码完成或发生错误，或者没有更多数据需要解码，则返回
            if ret != LzmaRet::Ok || *out_pos == out_size || coder.dict.pos < coder.dict.size {
                return ret;
            }
        }
    }
}

pub fn lz_decode(
    coder_ptr: &mut CoderType,

    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::LzDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    if coder.next.code.is_none() {
        return decode_buffer(coder, input, in_pos, in_size, output, out_pos, out_size);
    }

    // 我们不是链中的最后一个编码器，需要将输入解码到临时缓冲区
    while *out_pos < out_size {
        // 如果临时缓冲区为空，则填充它
        if !coder.next_finished && coder.temp.pos == coder.temp.size {
            coder.temp.pos = 0;
            coder.temp.size = 0;

            let mut ret = LzmaRet::Ok;
            if let Some(code) = coder.next.code {
                ret = code(
                    &mut coder.next.coder.as_mut().unwrap(),
                    input,
                    in_pos,
                    in_size,
                    &mut coder.temp.buffer,
                    &mut coder.temp.size,
                    LZMA_BUFFER_SIZE,
                    action.clone(),
                );
            }

            if ret == LzmaRet::StreamEnd {
                coder.next_finished = true;
            } else if ret != LzmaRet::Ok || coder.temp.size == 0 {
                return ret;
            }
        }

        if coder.this_finished {
            if coder.temp.size != 0 {
                return LzmaRet::DataError;
            }

            if coder.next_finished {
                return LzmaRet::StreamEnd;
            }

            return LzmaRet::Ok;
        }

        // 使用临时变量避免重复借用
        let mut temp_pos = coder.temp.pos;
        let temp_size = coder.temp.size;
        // SAFETY: decode_buffer only accesses coder.dict and coder.lz, not coder.temp.buffer
        let temp_buf: *mut [u8; LZMA_BUFFER_SIZE] = &mut coder.temp.buffer;
        let ret = decode_buffer(
            coder,
            unsafe { &mut *temp_buf },
            &mut temp_pos, // 使用临时变量
            temp_size,     // 使用临时变量
            output,
            out_pos,
            out_size,
        );

        if ret == LzmaRet::StreamEnd {
            coder.this_finished = true;
        } else if ret != LzmaRet::Ok {
            return ret;
        } else if coder.next_finished && *out_pos < out_size {
            return LzmaRet::DataError;
        }
    }

    LzmaRet::Ok
}

pub fn lz_decoder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::LzDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // 结束下一个编码器
    lzma_next_end(&mut coder.next);

    // 结束当前编码器
    if let Some(end_fn) = coder.lz.end {
        end_fn(coder.lz.coder.as_mut().unwrap());
    }
}

pub fn lzma_lz_decoder_init(
    next: &mut LzmaNextCoder,
    filters: &[LzmaFilterInfo],
    lz_init: fn(
        &mut LzmaLzDecoder,
        LzmaVli,
        &LzmaOptionsType,
        &mut LzmaLzDecoderOptions,
    ) -> LzmaRet,
) -> LzmaRet {
    // 如果编码器尚未分配，则进行分配
    if next.coder.is_none() {
        // 创建新的解码器实例
        let coder = LzmaDecoder::default();

        // 设置函数指针
        next.code = Some(lz_decode);
        next.end = Some(lz_decoder_end);
        next.coder = Some(CoderType::LzDecoder(coder));
    }

    // 获取解码器实例
    let coder = match next.coder.as_mut().unwrap() {
        CoderType::LzDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError,
    };

    // 初始化解码器字段
    coder.dict.buf = Vec::new();
    coder.dict.size = 0;
    coder.lz = LzmaLzDecoder::default();
    coder.next = Box::new(LzmaNextCoder::default());

    // 分配并初始化基于 LZ 的解码器，也会返回字典大小
    let mut lz_options = LzmaLzDecoderOptions {
        dict_size: 0,
        preset_dict: Vec::new(),
        preset_dict_size: 0,
    };

    let ret = lz_init(
        &mut coder.lz,
        filters[0].id,
        &filters[0].options.clone().unwrap(),
        &mut lz_options,
    );
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 如果字典大小非常小，则增加到 4096 字节
    if lz_options.dict_size < 4096 {
        lz_options.dict_size = 4096;
    }

    // 使字典大小为 16 的倍数
    if lz_options.dict_size > std::usize::MAX - 15 {
        return LzmaRet::MemError;
    }

    lz_options.dict_size = (lz_options.dict_size + 15) & !(15);

    // 分配并初始化字典
    if coder.dict.size != lz_options.dict_size {
        coder.dict.buf = vec![0; lz_options.dict_size];
        if coder.dict.buf.is_empty() {
            return LzmaRet::MemError;
        }
        coder.dict.size = lz_options.dict_size;
    }

    // 重置解码器
    lz_decoder_reset(coder);

    // 如果提供了预设字典，则使用它
    if !lz_options.preset_dict.is_empty() && lz_options.preset_dict_size > 0 {
        let copy_size = my_min(lz_options.preset_dict_size, lz_options.dict_size);
        let offset = lz_options.preset_dict_size - copy_size;
        coder
            .dict
            .buf
            .copy_from_slice(&lz_options.preset_dict[offset..offset + copy_size]);
        coder.dict.pos = copy_size;
        coder.dict.full = copy_size;
    }

    // 其他初始化
    coder.next_finished = false;
    coder.this_finished = false;
    coder.temp.pos = 0;
    coder.temp.size = 0;

    // 初始化链中的下一个过滤器（如果有的话）
    lzma_next_filter_init(&mut coder.next, &filters[1..])
}

pub fn lzma_lz_decoder_memusage(dictionary_size: usize) -> u64 {
    std::mem::size_of::<LzmaDecoder>() as u64 + dictionary_size as u64
}
