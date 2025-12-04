/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::common::NextCoderInitFunction;
use crate::{
    api::{
        LzmaAction, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream, LZMA_DICT_SIZE_MIN,
        LZMA_FILTER_LZMA1,
    },
    lzma::{lzma_lzma_encoder_init, lzma_lzma_lclppb_encode},
};
use common::write32le;

use super::{
    lzma_bufcpy, lzma_end, lzma_next_end, lzma_next_filter_init, lzma_strm_init, CoderType,
    LzmaFilterInfo, LzmaNextCoder,
};

const ALONE_HEADER_SIZE: usize = 1 + 4 + 8;

#[derive(Debug)]
pub struct LzmaAloneEncoder {
    next: Box<LzmaNextCoder>,
    sequence: Sequence,
    header_pos: usize,
    header: [u8; ALONE_HEADER_SIZE],
}
impl Default for LzmaAloneEncoder {
    fn default() -> Self {
        LzmaAloneEncoder {
            next: Box::new(LzmaNextCoder::default()), // Assuming LzmaNextCoder implements Default
            sequence: Sequence::default(),            // Assuming Sequence implements Default
            header_pos: 0,                            // Default value for usize
            header: [0u8; ALONE_HEADER_SIZE],         // Default array of zeros
        }
    }
}
#[derive(Debug, Default)]
enum Sequence {
    #[default]
    Header,
    Code,
}

fn alone_encode(
    coder_ptr: &mut CoderType,

    in_: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = coder_ptr;

    let coder = match coder_ptr {
        CoderType::AloneEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    while *out_pos < out_size {
        match coder.sequence {
            Sequence::Header => {
                // 将 &coder.header 转换为 *const u8
                //  let header_ptr = coder.header;

                lzma_bufcpy(
                    &mut coder.header,
                    &mut coder.header_pos,
                    ALONE_HEADER_SIZE,
                    out,
                    out_pos,
                    out_size,
                );
                if coder.header_pos < ALONE_HEADER_SIZE {
                    return LzmaRet::Ok;
                }
                coder.sequence = Sequence::Code;
            }
            Sequence::Code => {
                if let Some(code) = coder.next.code {
                    return code(
                        &mut coder.next.coder.as_mut().unwrap(),
                        in_,
                        in_pos,
                        in_size,
                        out,
                        out_pos,
                        out_size,
                        action,
                    );
                } else {
                    return LzmaRet::ProgError;
                }
            }
            _ => return LzmaRet::ProgError,
        }
    }

    LzmaRet::Ok
}

fn alone_encoder_end(coder_ptr: &mut CoderType) {
    // let coder = unsafe { &mut *(coder_ptr as *mut LzmaAloneEncoder) };
    let coder = match coder_ptr {
        CoderType::AloneDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };

    lzma_next_end(&mut coder.next);
}

fn alone_encoder_init(next: &mut LzmaNextCoder, options: &LzmaOptionsLzma) -> LzmaRet {
    // lzma_next_coder_init!(alone_encoder_init, next, Some(allocator));
    // 判断是否已经初始化过
    if next.init != Some(NextCoderInitFunction::AloneEncoder(alone_encoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::AloneEncoder(alone_encoder_init));

    let mut coder: &mut LzmaAloneEncoder = &mut LzmaAloneEncoder::default();
    if next.coder.is_none() {
        let coder_ = LzmaAloneEncoder {
            next: Box::new(LzmaNextCoder::default()),
            sequence: Sequence::Header,
            header_pos: 0,
            header: [0; ALONE_HEADER_SIZE],
        };
        next.code = Some(alone_encode);
        next.end = Some(alone_encoder_end);
        next.coder = Some(CoderType::AloneEncoder(coder_));
        coder.next = Box::new(LzmaNextCoder::default());
    }

    coder = match next.coder.as_mut().unwrap() {
        CoderType::AloneEncoder(ref mut c) => c,
        _ => {
            return LzmaRet::ProgError;
        } // 如果不是 AloneDecoder 类型，则返回错误
    };

    coder.sequence = Sequence::Header;
    coder.header_pos = 0;

    if lzma_lzma_lclppb_encode(options, &mut coder.header) {
        return LzmaRet::OptionsError;
    }

    if options.dict_size < LZMA_DICT_SIZE_MIN {
        return LzmaRet::OptionsError;
    }

    let mut d = options.dict_size - 1;
    d |= d >> 2;
    d |= d >> 3;
    d |= d >> 4;
    d |= d >> 8;
    d |= d >> 16;
    if d != u32::MAX {
        d += 1;
    }

    write32le(&mut coder.header[1..], d);

    coder.header[1 + 4..1 + 4 + 8].fill(0xFF);

    let filters = [
        LzmaFilterInfo {
            id: LZMA_FILTER_LZMA1,
            init: Some(lzma_lzma_encoder_init),
            options: Some(LzmaOptionsType::LzmaOptionsLzma(options.clone())),
        },
        LzmaFilterInfo {
            id: 0,
            init: None,
            options: None,
        },
    ];

    lzma_next_filter_init(&mut coder.next, &filters)
}

pub fn lzma_alone_encoder(strm: &mut LzmaStream, options: &LzmaOptionsLzma) -> LzmaRet {
    let ret_: LzmaRet = lzma_strm_init(Some(strm));
    if ret_ != LzmaRet::Ok {
        return ret_;
    }

    let mut internal = strm.internal.borrow_mut();
    if let Some(ref mut internal) = *internal {
        if let Some(ref mut next) = internal.next {
            let ret_0: LzmaRet = alone_encoder_init(next, options);
            if ret_0 != LzmaRet::Ok {
                drop(internal);
                // lzma_end(Some(strm));
                return ret_0;
            }

            internal.supported_actions[LzmaAction::Run as usize] = true;
            internal.supported_actions[LzmaAction::Finish as usize] = true;

            LzmaRet::Ok
        } else {
            drop(internal);
            // lzma_end(Some(strm));
            LzmaRet::ProgError
        }
    } else {
        drop(internal);
        // lzma_end(Some(strm));
        LzmaRet::ProgError
    }
}
