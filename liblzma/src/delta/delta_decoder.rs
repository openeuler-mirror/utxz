/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaAction, LzmaDeltaType, LzmaOptionsDelta, LzmaOptionsType, LzmaRet},
    common::{CoderType, LzmaFilterInfo, LzmaNextCoder},
};

use super::{lzma_delta_coder_init, LzmaDeltaCoder};

fn decode_buffer(coder: &mut LzmaDeltaCoder, buffer: &mut Vec<u8>, size: usize) {
    let distance = coder.distance;

    for i in 0..size {
        buffer[i] = buffer[i].wrapping_add(coder.history[(distance + coder.pos as usize) & 0xFF]);
        coder.history[coder.pos as usize & 0xFF] = buffer[i];
        coder.pos = coder.pos.wrapping_sub(1);
    }
}

fn delta_decode(
    coder_ptr: &mut CoderType,

    in_: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::DeltaCoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    assert!(coder.next.code.is_some());

    let out_start = *out_pos;

    let mut ret: LzmaRet = LzmaRet::Ok;
    if let Some(code) = coder.next.code {
        ret = code(
            &mut coder.next.coder.as_mut().unwrap(),
            in_,
            in_pos,
            in_size,
            out,
            out_pos,
            out_size,
            action,
        );
    }

    let size = *out_pos - out_start;
    if size > 0 {
        // 创建从 out_start 开始的切片
        let out_slice = &mut out[out_start..*out_pos];
        decode_buffer(coder, &mut out_slice.to_vec(), size);
    }
    ret
}

pub fn lzma_delta_decoder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    next.code = Some(delta_decode);
    lzma_delta_coder_init(next, filters)
}

pub fn lzma_delta_props_decode(
    // mut options: Option<LzmaOptionsType>,
    props: &[u8],
    props_size: usize,
) -> (LzmaRet, Option<LzmaOptionsType>) {
    if props_size != 1 {
        return (LzmaRet::OptionsError, None);
    }

    let opt = &mut LzmaOptionsDelta::default();

    opt.type_ = LzmaDeltaType::Byte;
    opt.dist = props[0] as u32 + 1;

    // 将 opt 包装成 Box<dyn Any>

    (LzmaRet::Ok, Some(LzmaOptionsType::Lod(opt.clone())))
}
