/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaAction, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream, LZMA_FILTER_LZMA1},
    common::NextCoderInitFunction,
    lzma::{lzma_lzma_encoder_init, lzma_lzma_lclppb_encode},
};

use super::{
    lzma_end, lzma_next_end, lzma_next_filter_init, lzma_strm_init, CoderType, LzmaFilterInfo,
    LzmaNextCoder,
};

/// MicroLZMA 编码器结构体
#[derive(Debug, Default)]
pub struct LzmaMicrolzmaEncoder {
    /// LZMA1 编码器
    lzma: Box<LzmaNextCoder>,

    /// LZMA 属性字节 (lc/lp/pb)
    props: u8,
}

fn microlzma_encode(
    coder_ptr: &mut CoderType,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_ref::<LzmaMicrolzmaEncoder>().unwrap();

    let coder = match coder_ptr {
        CoderType::MicroLzamEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // 记住起始输出位置，以便后续用 LZMA 属性字节覆盖第一个字节
    let out_start = *out_pos;

    // 记住起始输入位置，用于根据实际编码的未压缩字节数设置它
    let in_start = *in_pos;

    // 根据可用输出空间设置输出大小限制
    // 我们知道编码器支持 set_out_limit()，所以不可能返回 LZMA_OPTIONS_ERROR
    // LZMA_BUF_ERROR 是可能的，但 lzma_code() 有断言不允许从这里返回它
    // 所以 LZMA_BUF_ERROR 变成 LZMA_PROG_ERROR
    let mut uncomp_size = 0u64;

    if let Some(set_out_limit) = coder.lzma.set_out_limit {
        if set_out_limit(
            &mut coder.lzma.coder.as_mut().unwrap(),
            &mut uncomp_size,
            (out_size - *out_pos) as u64,
        ) != LzmaRet::Ok
        {
            return LzmaRet::ProgError;
        }
    }

    // set_out_limit 如果这不为真则会失败
    assert!(out_size - *out_pos >= 6);

    // 尽可能多地编码
    let mut ret = LzmaRet::Ok;
    if let Some(code) = coder.lzma.code {
        ret = code(
            &mut coder.lzma.coder.as_mut().unwrap(),
            input,
            in_pos,
            in_size,
            out,
            out_pos,
            out_size,
            action,
        );
    }

    match ret {
        LzmaRet::StreamEnd => {
            // 第一个输出字节是属性字节的按位取反
            // 我们知道这个字节有空间，因为 set_out_limit 和实际编码都成功了
            out[out_start] = !coder.props;

            // LZMA 编码器可能读取了比它能编码的更多输入
            // 根据 uncomp_size 设置 *in_pos
            assert!(uncomp_size <= (in_size - in_start) as u64);
            *in_pos = in_start + uncomp_size as usize;

            ret
        }
        LzmaRet::Ok => {
            assert!(false);
            LzmaRet::ProgError
        }
        _ => ret,
    }
}

/// 结束 MicroLZMA 解码器
fn microlzma_encoder_end(mut coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::MicroLzamEncoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.lzma);
}

fn microlzma_encoder_init(next: &mut LzmaNextCoder, options: &LzmaOptionsLzma) -> LzmaRet {
    // lzma_next_coder_init(next, allocator);
    if next.init
        != Some(NextCoderInitFunction::MicroLzamEncoder(
            microlzma_encoder_init,
        ))
    {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::MicroLzamEncoder(
        microlzma_encoder_init,
    ));

    let mut coder = &mut LzmaMicrolzmaEncoder::default();

    if next.coder.is_none() {
        let coder_ = LzmaMicrolzmaEncoder::default();

        next.coder = Some(CoderType::MicroLzamEncoder(coder_));
        next.code = Some(microlzma_encode);
        next.end = Some(microlzma_encoder_end);
        coder.lzma = Box::new(LzmaNextCoder::default());
    } else {
        coder = match next.coder.as_mut().unwrap() {
            CoderType::MicroLzamEncoder(ref mut c) => c,
            _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
        };
    }

    // 编码属性字节。它的按位取反将作为第一个输出字节
    if lzma_lzma_lclppb_encode(options, &mut [coder.props]) {
        return LzmaRet::OptionsError;
    }

    // 初始化 LZMA 编码器
    // let mut tmp: Box<dyn std::any::Any> = Box::new(options);
    let filters: [LzmaFilterInfo; 2] = [
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

    lzma_next_filter_init(&mut coder.lzma, &filters)
}

#[no_mangle]
pub fn lzma_microlzma_encoder(strm: &mut LzmaStream, options: &LzmaOptionsLzma) -> LzmaRet {
    // lzma_next_strm_init(microlzma_encoder_init, strm, options);
    let ret: LzmaRet = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 避免借用冲突的初始化
    let init_ret = match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                if let Some(ref mut next) = internal.next {
                    microlzma_encoder_init(next, options)
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
                internal.supported_actions[LzmaAction::Finish as usize] = true;
            }
        }
        Err(_) => return LzmaRet::ProgError,
    }

    LzmaRet::Ok
}
