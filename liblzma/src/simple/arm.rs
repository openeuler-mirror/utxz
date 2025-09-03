/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */


///////////////////////////////////////////////////////////////////////////////
//
/// \file       arm.rs
/// \brief      ARM二进制文件的过滤器
///
//  Authors:    Igor Pavlov
//              Lasse Collin
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////
use crate::{
    api::{LzmaFilter, LzmaRet},
    common::{LzmaFilterInfo, LzmaNextCoder},
};

use super::{lzma_simple_coder_init, simple_private::*};

/// ARM代码过滤器实现
fn arm_code(
    _simple: &mut SimpleType, // 使用Option替代void指针
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let mut i = 0;

    // 每次处理4字节
    while i + 4 <= size {
        if buffer[i + 3] == 0xEB {
            // 从小端字节序中构建32位值
            let mut src =
                ((buffer[i + 2] as u32) << 16) | ((buffer[i + 1] as u32) << 8) | (buffer[i] as u32);
            src <<= 2;

            let dest = if is_encoder {
                // 编码时计算目标地址
                now_pos + (i as u32) + 8 + src
            } else {
                // 解码时计算源地址
                src.wrapping_sub(now_pos + (i as u32) + 8)
            };

            // 将目标地址转换回字节并存储
            let dest = dest >> 2;
            buffer[i + 2] = (dest >> 16) as u8;
            buffer[i + 1] = (dest >> 8) as u8;
            buffer[i] = dest as u8;
        }
        i += 4;
    }

    i
}

/// 初始化ARM编码器
fn arm_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    // 使用简单编码器初始化
    // 参数说明：
    // - 0: 不需要额外的过滤器数据
    // - 4: 未过滤数据的最大大小
    // - 4: 对齐要求
    lzma_simple_coder_init(next, filters, arm_code, 0, 4, 4, is_encoder)
}

/// ARM编码器初始化函数

pub fn lzma_simple_arm_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    arm_coder_init(next, filters, true)
}

/// ARM解码器初始化函数

pub fn lzma_simple_arm_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    arm_coder_init(next, filters, false)
}
