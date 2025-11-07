/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::{LzmaRet, LZMA_VLI_BYTES_MAX, LZMA_VLI_MAX};

/// 编码可变长度整数
///
/// # 参数
/// * `vli` - 要编码的整数值
/// * `vli_pos` - 当前编码位置，单次调用模式下为 `None`
/// * `out` - 输出缓冲区
/// * `out_pos` - 输出缓冲区的当前位置
/// * `out_size` - 输出缓冲区的大小
pub fn lzma_vli_encode(
    mut vli: u64,
    mut vli_pos: Option<&mut usize>,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 处理单次调用模式与流模式
    let mut vli_pos_internal = 0;
    let vli_pos_ref = if let Some(pos) = vli_pos.as_mut() {
        pos
    } else {
        &mut vli_pos_internal
    };
    if *out_pos >= out_size {
        return if vli_pos.is_none() {
            LzmaRet::ProgError
        } else {
            LzmaRet::BufError
        };
    }
    // 验证参数
    if *vli_pos_ref >= LZMA_VLI_BYTES_MAX || vli > LZMA_VLI_MAX {
        return LzmaRet::ProgError;
    }
    // 将 vli 右移，使得要编码的位成为最低位
    vli >>= *vli_pos_ref * 7;
    // 在循环中写入非最后一个字节
    while vli >= 0x80 {
        *vli_pos_ref += 1;
        assert!(*vli_pos_ref < LZMA_VLI_BYTES_MAX);
        // 写入下一个字节
        out[*out_pos] = (vli as u8) | 0x80;
        vli >>= 7;
        *out_pos += 1;
        if *out_pos == out_size {
            return if vli_pos.is_none() {
                LzmaRet::ProgError
            } else {
                LzmaRet::Ok
            };
        }
    }
    // 写入最后一个字节
    out[*out_pos] = vli as u8;
    *out_pos += 1;
    *vli_pos_ref += 1;

    if vli_pos.is_none() {
        LzmaRet::Ok
    } else {
        LzmaRet::StreamEnd
    }
}
