/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       ia64.rs
/// \brief      IA64 (Itanium)二进制文件的过滤器
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

/// 分支表常量
const BRANCH_TABLE: [u32; 32] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 4, 6, 6, 0, 0, 7, 7, 4, 4, 0, 0, 4, 4, 0, 0,
];

/// IA64代码过滤器实现
fn ia64_code(
    _simple: &mut SimpleType,
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let mut i = 0;

    // 每次处理16字节
    while i + 16 <= size {
        let instr_template = buffer[i] & 0x1F;
        let mask = BRANCH_TABLE[instr_template as usize];
        let mut bit_pos = 5u32;

        // 处理3个槽位
        for slot in 0..3 {
            if ((mask >> slot) & 1) == 0 {
                bit_pos += 41;
                continue;
            }

            let byte_pos = (bit_pos >> 3) as usize;
            let bit_res = bit_pos & 0x7;
            let mut instruction: u64 = 0;

            // 从6个字节构建指令
            for j in 0..6 {
                instruction |= (buffer[i + j + byte_pos] as u64) << (8 * j);
            }

            let mut inst_norm = instruction >> bit_res;

            // 检查指令格式
            if ((inst_norm >> 37) & 0xF) == 0x5 && ((inst_norm >> 9) & 0x7) == 0 {
                let mut src = ((inst_norm >> 13) & 0xFFFFF) as u32;
                src |= (((inst_norm >> 36) & 1) << 20) as u32;

                src <<= 4;

                // 计算目标地址
                let dest = if is_encoder {
                    now_pos.wrapping_add(i as u32).wrapping_add(src)
                } else {
                    src.wrapping_sub(now_pos.wrapping_add(i as u32))
                };

                let dest = dest >> 4;

                // 更新指令
                inst_norm &= !((0x8FFFFF as u64) << 13);
                inst_norm |= ((dest & 0xFFFFF) as u64) << 13;
                inst_norm |= ((dest & 0x100000) as u64) << (36 - 20);

                instruction &= (1u64 << bit_res) - 1;
                instruction |= inst_norm << bit_res;

                // 写回6个字节
                for j in 0..6 {
                    buffer[i + j + byte_pos] = ((instruction >> (8 * j)) & 0xFF) as u8;
                }
            }

            bit_pos += 41;
        }

        i += 16;
    }

    i
}

/// 初始化IA64编码器
fn ia64_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    // 使用简单编码器初始化
    // 参数说明：
    // - 0: 不需要额外的过滤器数据
    // - 16: 未过滤数据的最大大小
    // - 16: 对齐要求
    lzma_simple_coder_init(next, filters, ia64_code, 0, 16, 16, is_encoder)
}

/// IA64编码器初始化函数
pub fn lzma_simple_ia64_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    ia64_coder_init(next, filters, true)
}

/// IA64解码器初始化函数
pub fn lzma_simple_ia64_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    ia64_coder_init(next, filters, false)
}
