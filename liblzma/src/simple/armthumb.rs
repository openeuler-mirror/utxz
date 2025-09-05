/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       armthumb.rs
/// \brief      ARM-Thumb二进制文件的过滤器
///
//  Authors:    Igor Pavlov
//              Lasse Collin
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////
use crate::{
    api::LzmaRet,
    common::{LzmaFilterInfo, LzmaNextCoder},
};

use super::{lzma_simple_coder_init, simple_private::*};

/// ARM-Thumb代码过滤器实现
fn armthumb_code(
    _simple: &mut SimpleType,
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let mut i = 0;

    // 每次处理4字节，但步进2字节
    while i + 4 <= size {
        // 检查Thumb指令的特征模式
        if (buffer[i + 1] & 0xF8) == 0xF0 && (buffer[i + 3] & 0xF8) == 0xF8 {
            // 从四个字节中构建32位源地址
            let mut src = (((buffer[i + 1] as u32) & 7) << 19)
                | ((buffer[i] as u32) << 11)
                | (((buffer[i + 3] as u32) & 7) << 8)
                | (buffer[i + 2] as u32);

            // 左移一位
            src <<= 1;

            // 计算目标地址
            let dest = if is_encoder {
                // 编码时：目标 = 当前位置 + 偏移 + 4 + 源地址
                now_pos
                    .wrapping_add(i as u32)
                    .wrapping_add(4)
                    .wrapping_add(src)
            } else {
                // 解码时：目标 = 源地址 - (当前位置 + 偏移 + 4)
                src.wrapping_sub(now_pos.wrapping_add(i as u32).wrapping_add(4))
            };

            // 右移一位并更新缓冲区
            let dest = dest >> 1;
            buffer[i + 1] = 0xF0 | ((dest >> 19) & 0x7) as u8;
            buffer[i] = (dest >> 11) as u8;
            buffer[i + 3] = 0xF8 | ((dest >> 8) & 0x7) as u8;
            buffer[i + 2] = dest as u8;

            // 额外跳过2字节，因为已经处理了4字节
            i += 2;
        }

        // 正常步进2字节
        i += 2;
    }

    i
}

/// 初始化ARM-Thumb编码器
fn armthumb_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    // 使用简单编码器初始化
    // 参数说明：
    // - 0: 不需要额外的过滤器数据
    // - 4: 未过滤数据的最大大小
    // - 2: 对齐要求（Thumb模式使用2字节对齐）
    lzma_simple_coder_init(next, filters, armthumb_code, 0, 4, 2, is_encoder)
}

/// ARM-Thumb编码器初始化函数

pub fn lzma_simple_armthumb_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    armthumb_coder_init(next, filters, true)
}

/// ARM-Thumb解码器初始化函数

pub fn lzma_simple_armthumb_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    armthumb_coder_init(next, filters, false)
}
