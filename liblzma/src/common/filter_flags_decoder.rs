/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

 use crate::api::{LzmaFilter, LzmaRet, LzmaVli};

use super::{lzma_properties_decode, lzma_vli_decode, LZMA_FILTER_RESERVED_START};

/// 解码过滤器标志
pub fn lzma_filter_flags_decode(
    filter: &mut LzmaFilter,

    in_data: &mut [u8],
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    // 将指针设为 None，以便调用者可以安全地释放它。
    filter.options = None;

    // 解码过滤器 ID
    let id_ret = lzma_vli_decode(&mut filter.id, None, in_data, in_pos, in_size);
    if id_ret != LzmaRet::Ok {
        return id_ret;
    }

    // 检查是否是保留的 ID
    if filter.id >= LZMA_FILTER_RESERVED_START {
        return LzmaRet::DataError;
    }

    // 解码属性的大小
    let mut props_size: LzmaVli = 0;
    let props_size_ret = lzma_vli_decode(&mut props_size, None, in_data, in_pos, in_size);
    if props_size_ret != LzmaRet::Ok {
        return props_size_ret;
    }

    // 解码属性
    if in_size - *in_pos < props_size as usize {
        return LzmaRet::DataError;
    }

    // 解码属性并更新位置
    let props_data = &mut in_data[*in_pos..*in_pos + props_size as usize];
    let ret = lzma_properties_decode(filter, props_data, props_size as usize);
    *in_pos += props_size as usize;

    ret
}
