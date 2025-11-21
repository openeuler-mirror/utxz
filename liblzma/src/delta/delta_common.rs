/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */


use std::any::Any;

use common::memzero;

use crate::{
    api::{
        LzmaDeltaType, LzmaOptionsDelta, LzmaOptionsType, LzmaRet, LZMA_DELTA_DIST_MAX,
        LZMA_DELTA_DIST_MIN,
    },
    common::{lzma_next_end, lzma_next_filter_init, CoderType, LzmaFilterInfo, LzmaNextCoder},
};

use super::LzmaDeltaCoder;

fn delta_coder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::DeltaCoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(coder.next.as_mut());
}

pub fn lzma_delta_coder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    let mut coder = LzmaDeltaCoder::default();
    if next.coder.is_none() {
        let coder_ = LzmaDeltaCoder::default();
        next.end = Some(delta_coder_end);
        coder.next = Box::new(LzmaNextCoder::default());
        next.coder = Some(CoderType::DeltaCoder(coder_));
    }

    if lzma_delta_coder_memusage(&filters[0].options.clone().unwrap()) == u64::MAX {
        return LzmaRet::OptionsError;
    }

    let opt = match filters[0].options.as_ref().unwrap() {
        LzmaOptionsType::Delta(c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    coder.distance = opt.dist as usize;

    coder.pos = 0;

    memzero(&mut coder.history);

    lzma_next_filter_init(&mut coder.next, std::slice::from_ref(&filters[1]))
}

pub fn lzma_delta_coder_memusage(mut options: &LzmaOptionsType) -> u64 {
    let opt = match options {
        LzmaOptionsType::Delta(c) => c,
        _ => return 0, // 如果不是 AloneDecoder 类型，则返回错误
    };
    if opt.type_ != LzmaDeltaType::Byte
        || opt.dist < LZMA_DELTA_DIST_MIN
        || opt.dist > LZMA_DELTA_DIST_MAX
    {
        return return u64::MAX;
    }

    std::mem::size_of::<LzmaDeltaCoder>() as u64
}
