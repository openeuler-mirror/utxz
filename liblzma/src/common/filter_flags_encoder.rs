/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaFilter, LzmaRet};

use super::{
    lzma_properties_encode, lzma_properties_size, lzma_vli_encode, lzma_vli_size,
    LZMA_FILTER_RESERVED_START,
};

/// 计算过滤器标志的大小
pub fn lzma_filter_flags_size(size: &mut u32, filter: &LzmaFilter) -> LzmaRet {
    // 如果过滤器 ID 大于或等于保留 ID，返回程序错误
    if filter.id >= LZMA_FILTER_RESERVED_START {
        return LzmaRet::ProgError;
    }

    // 计算属性大小
    let props_size_ret = lzma_properties_size(size, filter);
    if props_size_ret != LzmaRet::Ok {
        return props_size_ret;
    }

    // 加上 ID 和属性大小
    *size += lzma_vli_size(filter.id) + lzma_vli_size(*size as u64);

    LzmaRet::Ok
}

/// 编码过滤器标志
pub fn lzma_filter_flags_encode(
    filter: &LzmaFilter,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 如果过滤器 ID 大于或等于保留 ID，返回程序错误
    if filter.id >= LZMA_FILTER_RESERVED_START {
        return LzmaRet::ProgError;
    }

    // 编码过滤器 ID
    let id_ret = lzma_vli_encode(filter.id, None, out, out_pos, out_size);
    if id_ret != LzmaRet::Ok {
        return id_ret;
    }

    // 计算属性大小并编码
    let mut props_size: u32 = 0;
    let props_size_ret = lzma_properties_size(&mut props_size, filter);
    if props_size_ret != LzmaRet::Ok {
        return props_size_ret;
    }

    let props_size_ret = lzma_vli_encode(props_size as u64, None, out, out_pos, out_size);
    if props_size_ret != LzmaRet::Ok {
        return props_size_ret;
    }

    // 编码属性
    if out_size - *out_pos < props_size as usize {
        return LzmaRet::ProgError;
    }

    let props_encode_ret = lzma_properties_encode(filter, &mut out[*out_pos..]);
    if props_encode_ret != LzmaRet::Ok {
        return props_encode_ret;
    }

    *out_pos += props_size as usize;

    LzmaRet::Ok
}
