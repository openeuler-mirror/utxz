/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

 use common::my_min;

use crate::{
    api::{
        LzmaAction, LzmaFilter, LzmaOptionsDelta, LzmaOptionsType, LzmaRet, LZMA_DELTA_DIST_MIN,
    },
    common::{lzma_next_filter_update, CoderType, LzmaFilterInfo, LzmaNextCoder},
};

use super::{lzma_delta_coder_init, lzma_delta_coder_memusage, LzmaDeltaCoder};

fn copy_and_encode(coder: &mut LzmaDeltaCoder, in_: &Vec<u8>, out: &mut [u8], size: usize) {
    let distance = coder.distance;

    for i in 0..size {
        let tmp = coder.history[(distance + coder.pos as usize) & 0xFF];
        coder.history[coder.pos as usize & 0xFF] = in_[i];
        coder.pos = coder.pos.wrapping_sub(1);
        out[i] = in_[i].wrapping_sub(tmp);
    }
}

fn encode_in_place(coder: &mut LzmaDeltaCoder, buffer: &mut [u8], size: usize) {
    let distance = coder.distance;

    for i in 0..size {
        let tmp = coder.history[(distance + coder.pos as usize) & 0xFF];
        coder.history[coder.pos as usize & 0xFF] = buffer[i];
        coder.pos = coder.pos.wrapping_sub(1);
        buffer[i] = buffer[i].wrapping_sub(tmp);
    }
}

fn delta_encode(
    coder_ptr: &mut CoderType,

    in_: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = unsafe { &mut *(coder_ptr as *mut LzmaDeltaCoder) };
    let coder = match coder_ptr {
        CoderType::DeltaCoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    let mut ret: LzmaRet = LzmaRet::Ok;

    if coder.next.code.is_none() {
        let in_avail = in_size - *in_pos;
        let out_avail = out_size - *out_pos;
        let size = my_min(in_avail, out_avail);

        if size > 0 {
            copy_and_encode(coder, in_, out, size);
        }

        *in_pos += size;
        *out_pos += size;

        ret = if action != LzmaAction::Run && *in_pos == in_size {
            LzmaRet::StreamEnd
        } else {
            LzmaRet::Ok
        };
    } else {
        let out_start = *out_pos;

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
            )
        }

        let size = *out_pos - out_start;
        if size > 0 {
            encode_in_place(coder, out, size);
        }
    }

    ret
}

fn delta_encoder_update(
    coder_ptr: &mut CoderType,

    filters_null: Option<&[LzmaFilter]>,
    reversed_filters: &[LzmaFilter],
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::DeltaCoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    return lzma_next_filter_update(&mut coder.next, std::slice::from_ref(&reversed_filters[1]));
}

pub fn lzma_delta_encoder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    next.code = Some(delta_encode);
    next.update = Some(delta_encoder_update);
    lzma_delta_coder_init(next, filters)
}

pub fn lzma_delta_props_encode(options: &LzmaOptionsType, out: &mut [u8]) -> LzmaRet {
    if lzma_delta_coder_memusage(options) == u64::MAX {
        return LzmaRet::ProgError;
    }
    let opt = match options {
        LzmaOptionsType::Delta(c) => c,
        _ => return LzmaRet::ProgError,
    };
    // let opt = options.downcast_mut::<&mut LzmaOptionsDelta>().unwrap();
    out[0] = (opt.dist - LZMA_DELTA_DIST_MIN) as u8;

    LzmaRet::Ok
}
