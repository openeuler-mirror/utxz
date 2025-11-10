/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_min;

use crate::{
    api::{LzmaAction, LzmaOptionsType, LzmaRet, LzmaVli},
    common::{
        lzma_bufcpy, lzma_next_end, lzma_next_filter_init, CoderType, LzmaFilterInfo,
        LzmaNextCoder, LZMA_BUFFER_SIZE,
    },
    lzma::{self, LzmaLzma1Decoder, LzmaLzma2Decoder, LzmaLzma2Encoder},
};

#[derive(Debug, Clone, Default)]
pub struct LzmaDict {
    pub buf: Vec<u8>,
    pub pos: usize,
    pub full: usize,
    pub limit: usize,
    pub size: usize,
    pub need_reset: bool,
}
// impl Default for LzmaDict {
//     fn default() -> Self {
//         // 使用一个空的 Vec<u8> 来初始化 buf
//         LzmaDict {
//             buf: Vec::new(), // 初始化为一个空 Vec
//             pos: 0,
//             full: 0,
//             limit: 0,
//             size: 0,
//             need_reset: false,
//         }
//     }
// }
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

#[derive(Debug, Default)]
pub struct LzmaLzDecoder {
    pub coder: Option<LzCoderType>,
    pub code: Option<
        fn(
            coder: &mut LzCoderType,
            dict: &mut LzmaDict,
            in_: &Vec<u8>,
            in_pos: &mut usize,
            in_size: usize,
        ) -> LzmaRet,
    >,
    pub reset: Option<fn(coder: &mut LzCoderType, options: &LzmaOptionsType)>,
    pub set_uncompressed:
        Option<fn(coder: &mut LzCoderType, uncompressed_size: LzmaVli, allow_eopm: bool)>,
    pub end: Option<fn(coder: &mut LzCoderType)>,
}

// impl Default for LzmaLzDecoder {
//     fn default() -> Self {
//         LzmaLzDecoder {
//             coder: None,            // 假设 LzCoderType 实现了 Default
//             code: None,             // Option 类型默认是 None
//             reset: None,            // Option 类型默认是 None
//             set_uncompressed: None, // Option 类型默认是 None
//             end: None,              // Option 类型默认是 None
//         }
//     }
// }

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
        // 源和目标区域重叠，逐字节复制
        while left > 0 {
            dict.buf[dict.pos] = dict_get(dict, distance as u32);
            dict.pos += 1;
            left -= 1;
        }
    } else if distance < dict.pos {
        // 最简单和最快的情况，直接复制
        let start = dict.pos - distance - 1;
        let end = start + left;
        dict.buf.copy_within(start..end, dict.pos);
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

#[inline]
pub fn dict_write(
    dict: &mut LzmaDict,
    input: &Vec<u8>,
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
    input: &Vec<u8>,
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
            output[*out_pos..*out_pos + copy_size]
                .copy_from_slice(&coder.dict.buf[dict_start..dict_start + copy_size]);
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
