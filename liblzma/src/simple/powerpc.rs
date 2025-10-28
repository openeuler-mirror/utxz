/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       powerpc.rs
/// \brief      PowerPC (大端序) 二进制文件的过滤器
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

/// PowerPC代码过滤器实现
fn powerpc_code(
    _simple: &mut SimpleType,
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let mut i = 0;

    // 每次处理4字节
    while i + 4 <= size {
        // PowerPC分支指令格式：6(48) 24(Offset) 1(Abs) 1(Link)
        // 检查指令是否为分支指令
        if (buffer[i] >> 2) == 0x12 && ((buffer[i + 3] & 3) == 1) {
            // 从大端字节序中构建32位源地址
            let src = (((buffer[i] as u32) & 3) << 24)
                | ((buffer[i + 1] as u32) << 16)
                | ((buffer[i + 2] as u32) << 8)
                | ((buffer[i + 3] as u32) & !3);

            // 计算目标地址
            let dest = if is_encoder {
                // 编码时：目标 = 当前位置 + 偏移 + 源地址
                now_pos.wrapping_add(i as u32).wrapping_add(src)
            } else {
                // 解码时：目标 = 源地址 - (当前位置 + 偏移)
                src.wrapping_sub(now_pos.wrapping_add(i as u32))
            };

            // 更新缓冲区中的指令
            buffer[i] = 0x48 | ((dest >> 24) & 0x03) as u8;
            buffer[i + 1] = (dest >> 16) as u8;
            buffer[i + 2] = (dest >> 8) as u8;
            buffer[i + 3] = (buffer[i + 3] & 0x03) | (dest as u8);
        }

        i += 4;
    }

    i
}

/// 初始化PowerPC编码器
fn powerpc_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    // 使用简单编码器初始化
    // 参数说明：
    // - 0: 不需要额外的过滤器数据
    // - 4: 未过滤数据的最大大小
    // - 4: 对齐要求
    lzma_simple_coder_init(next, filters, powerpc_code, 0, 4, 4, is_encoder)
}

/// PowerPC编码器初始化函数

pub fn lzma_simple_powerpc_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    powerpc_coder_init(next, filters, true)
}

/// PowerPC解码器初始化函数

pub fn lzma_simple_powerpc_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    powerpc_coder_init(next, filters, false)
}
