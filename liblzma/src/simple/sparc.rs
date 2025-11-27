/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */


// sparc.rs

// 假设以下模块、结构体、枚举、函数已在其它文件中定义：
// - crate::api::LzmaFilterInfo
// - crate::common::{LzmaNextCoder, LzmaRet, LzmaAllocator, lzma_simple_coder_init}
// - 其它有关宏和结构体，如 LzmaNextCoder::default(), etc.

use crate::{
    api::LzmaRet,
    common::{LzmaFilterInfo, LzmaNextCoder},
};

use super::{lzma_simple_coder_init, SimpleType};

// 定义一个常量（本 C 代码中没有显式常量，但此处为示例）
const FOUR: usize = 4;

/// SPARC过滤器代码函数
///
/// 此函数用于对SPARC二进制数据进行过滤转换。
/// 参数说明：
/// - `_simple`: 占位参数（未使用）
/// - `now_pos`: 当前数据的绝对位置（u32类型）
/// - `is_encoder`: 是否为编码器模式（true表示编码器，false表示解码器）
/// - `buffer`: 待处理的数据缓冲区（可变字节切片）
/// - `size`: 缓冲区大小
///
/// 返回值：处理过的字节数（每次处理4个字节）
pub fn sparc_code(
    _simple: &mut SimpleType,
    now_pos: u32,
    is_encoder: bool,
    buffer: &mut [u8],
    size: usize,
) -> usize {
    let mut i: usize = 0;
    // 循环遍历缓冲区，每4个字节为一组处理
    while i + FOUR <= size {
        // 判断是否满足特定模式：
        // 当 buffer[i] == 0x40 且 buffer[i+1] 的高2位为0，
        // 或 buffer[i] == 0x7F 且 buffer[i+1] 的高2位为 0xC0
        if (buffer[i] == 0x40 && (buffer[i + 1] & 0xC0) == 0x00)
            || (buffer[i] == 0x7F && (buffer[i + 1] & 0xC0) == 0xC0)
        {
            // 组装4个字节为32位整数（大端序）
            let mut src: u32 = ((buffer[i] as u32) << 24)
                | ((buffer[i + 1] as u32) << 16)
                | ((buffer[i + 2] as u32) << 8)
                | (buffer[i + 3] as u32);
            // 左移两位
            src <<= 2;

            // 根据编码器或解码器模式计算dest
            let dest: u32 = if is_encoder {
                now_pos.wrapping_add(i as u32).wrapping_add(src)
            } else {
                // 注意：使用wrapping_sub确保整数减法安全
                src.wrapping_sub(now_pos.wrapping_add(i as u32))
            };
            // 右移2位
            let mut dest = dest >> 2;

            // 计算最终的dest值：
            // 取 (dest >> 22) & 1, 若为1则其补码为 all 1，再左移22位后与0x3FFFFFFF相与，
            // 与 dest 的低22位相或，并置上0x40000000
            let part1 = (((0u32).wrapping_sub((dest >> 22) & 1)) << 22) & 0x3FFFFFFF;
            let part2 = dest & 0x3FFFFF;
            let dest = part1 | part2 | 0x40000000;

            // 分解dest为4个字节，并存回buffer
            buffer[i] = (dest >> 24) as u8;
            buffer[i + 1] = (dest >> 16) as u8;
            buffer[i + 2] = (dest >> 8) as u8;
            buffer[i + 3] = dest as u8;
        }
        i += FOUR;
    }
    i
}

/// sparc_coder_init函数：初始化SPARC编码/解码器
///
/// 参数：
/// - `next`: 下一个编码器/解码器（类型 LzmaNextCoder）
/// - `allocator`: 内存分配器（类型 LzmaAllocator）
/// - `filters`: 过滤器配置信息（类型 LzmaFilterInfo）
/// - `is_encoder`: 是否为编码器模式
///
/// 返回：LzmaRet 类型表示操作结果
pub fn sparc_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    is_encoder: bool,
) -> LzmaRet {
    // 调用已有的 lzma_simple_coder_init 函数
    lzma_simple_coder_init(next, filters, sparc_code, 0, 4, 4, is_encoder)
}

/// 当 feature "encoder_sparc" 激活时，定义 lzma_simple_sparc_encoder_init 函数
pub fn lzma_simple_sparc_encoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    sparc_coder_init(next, filters, true)
}

/// 当 feature "decoder_sparc" 激活时，定义 lzma_simple_sparc_decoder_init 函数
pub fn lzma_simple_sparc_decoder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
) -> LzmaRet {
    sparc_coder_init(next, filters, false)
}
