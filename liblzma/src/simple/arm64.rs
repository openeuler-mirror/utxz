/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       arm64.rs
/// \brief      ARM64二进制文件的过滤器
///
/// 此过滤器将ARM64的BL和ADRP指令中的相对地址转换为绝对值，
/// 以提高ARM64代码的冗余度。
///
/// 转换B或ADR指令也经过测试，但效果不佳。
/// B指令的大多数跳转都很小（+/- 0xFF）。
/// 这些通常用于循环和if语句。将它们编码为绝对地址会降低冗余度，
/// 因为许多小的相对跳转值会重复出现，但很少有绝对地址会重复。
//
//  Authors:    Lasse Collin
//              Jia Tan
//              Igor Pavlov
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
use byteorder::{ByteOrder, LittleEndian};

/// ARM64代码过滤器实现
fn arm64_code(
    _simple: &mut SimpleType,
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let mut i = 0;

    // 注意：在Rust中，我们不需要显式禁用向量化，
    // 因为Rust编译器会根据具体情况自动选择最优的实现
    while i + 4 <= size {
        let pc = now_pos + i as u32;
        let mut instr = LittleEndian::read_u32(&buffer[i..i + 4]);

        if (instr >> 26) == 0x25 {
            // BL指令：
            // 转换完整的26位立即数。
            // 范围是+/-128 MiB。
            //
            // 使用完整范围对大型可执行文件很有帮助。
            // 较小的范围会减少输入的非代码部分中的误报，
            // 所以这是一个稍微偏向大文件的折衷方案。
            // 使用完整范围时，32位中只需要6位匹配就能触发转换。
            let src = instr;
            instr = 0x94000000;

            let mut pc = pc >> 2;
            if !is_encoder {
                pc = pc.wrapping_neg();
            }

            instr |= (src.wrapping_add(pc)) & 0x03FF_FFFF;
            LittleEndian::write_u32(&mut buffer[i..i + 4], instr);
        } else if (instr & 0x9F00_0000) == 0x9000_0000 {
            // ADRP指令：
            // 只转换+/-512 MiB范围内的值。
            //
            // 使用小于完整+/-4 GiB范围可以减少输入的非代码部分的误报，
            // 同时对于512 MiB以下的可执行文件效果很好。
            // ADRP转换的正面效果比BL小，但在输入的非代码部分也不会造成太大伤害，
            // 因为在+/-512 MiB范围内，需要32位中的9位匹配才能触发转换
            // （两个10位匹配选项 = 9位）。
            let src = ((instr >> 29) & 3) | ((instr >> 3) & 0x001F_FFFC);

            // 通过加法只需要一个分支就可以检查+/-范围。
            // 在处理ARM64代码时这通常为false，
            // 所以分支预测在性能方面会处理得很好。
            if (src.wrapping_add(0x0002_0000)) & 0x001C_0000 != 0 {
                i += 4;
                continue;
            }

            instr &= 0x9000_001F;

            let mut pc = pc >> 12;
            if !is_encoder {
                pc = pc.wrapping_neg();
            }

            let dest = src.wrapping_add(pc);
            instr |= (dest & 3) << 29;
            instr |= (dest & 0x0003_FFFC) << 3;
            instr |= (0u32.wrapping_sub(dest & 0x0002_0000)) & 0x00E0_0000;
            LittleEndian::write_u32(&mut buffer[i..i + 4], instr);
        }

        i += 4;
    }

    i
}

/// 初始化ARM64编码器
fn arm64_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    lzma_simple_coder_init(next, filters, arm64_code, 0, 4, 4, is_encoder)
}

/// ARM64编码器初始化函数

pub fn lzma_simple_arm64_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    arm64_coder_init(next, filters, true)
}

/// ARM64解码器初始化函数

pub fn lzma_simple_arm64_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    arm64_coder_init(next, filters, false)
}
