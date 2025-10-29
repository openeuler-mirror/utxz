/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       simple_encoder.rs
/// \brief      简单过滤器的属性编码器
//
//  Author:     Lasse Collin
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////
use common::write32le;

use crate::api::{LzmaOptionsType, LzmaRet};

/// 计算简单过滤器属性的大小
///
/// # 参数
/// * `size` - 输出大小的可变引用
/// * `options` - BCJ选项的引用
///
/// # 返回值
/// * `LzmaRet` - 操作结果
pub fn lzma_simple_props_size(size: &mut u32, options: &LzmaOptionsType) -> LzmaRet {
    // 如果选项为空或起始偏移量为0，则不需要存储任何选项
    let opt = match options {
        LzmaOptionsType::Bcj(c) => c,
        _ => return LzmaRet::ProgError,
    };
    *size = match Some(opt) {
        Some(opt) if opt.start_offset != 0 => 4,
        _ => 0,
    };

    LzmaRet::Ok
}

/// 编码简单过滤器的属性
///
/// # 参数
/// * `options` - BCJ选项的引用
/// * `out` - 输出缓冲区
///
/// # 返回值
/// * `LzmaRet` - 操作结果
pub fn lzma_simple_props_encode(options: &LzmaOptionsType, out: &mut [u8]) -> LzmaRet {
    // 默认起始偏移量为零，所以除非起始偏移量非零，
    // 否则我们不需要存储任何选项
    let opt = match options {
        LzmaOptionsType::Bcj(c) => c,
        _ => return LzmaRet::ProgError,
    };

    if Some(opt).is_none() || opt.start_offset == 0 {
        return LzmaRet::Ok;
    }

    write32le(&mut out[1..], opt.start_offset);

    LzmaRet::Ok
}
