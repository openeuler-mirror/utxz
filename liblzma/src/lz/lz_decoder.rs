/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaOptionsType, LzmaRet, LzmaVli},
    common::{LzmaNextCoder, LZMA_BUFFER_SIZE},
    lzma::{LzmaLzma1Decoder, LzmaLzma2Decoder},
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
