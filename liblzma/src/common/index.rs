/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::{
    api::{LzmaVli, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_MAX},
    common::{IndexGroup, IndexRecord},
};

use super::lzma_vli_size;

/// 最小未填充大小
pub const UNPADDED_SIZE_MIN: LzmaVli = 5;

/// 最大未填充大小
pub const UNPADDED_SIZE_MAX: LzmaVli = LZMA_VLI_MAX & !3;

/// 根据 xz 规范的索引指示符
pub const INDEX_INDICATOR: u8 = 0;

/// 将可变长度整数向上舍入到四的倍数
pub fn vli_ceil4(vli: LzmaVli) -> LzmaVli {
    assert!(vli <= LZMA_VLI_MAX);
    (vli + 3) & !3
}

/// 计算索引字段的大小（不包括索引填充）
pub fn index_size_unpadded(count: LzmaVli, index_list_size: LzmaVli) -> LzmaVli {
    // 索引指示符 + 记录数量 + 记录列表 + CRC32
    1 + lzma_vli_size(count) as LzmaVli + index_list_size + 4
}

/// 计算索引字段的大小（包括索引填充）
pub fn index_size(count: LzmaVli, index_list_size: LzmaVli) -> LzmaVli {
    vli_ceil4(index_size_unpadded(count, index_list_size))
}

/// 计算流的总大小
pub fn index_stream_size(
    blocks_size: LzmaVli,
    count: LzmaVli,
    index_list_size: LzmaVli,
) -> LzmaVli {
    LZMA_STREAM_HEADER_SIZE as u64
        + blocks_size
        + index_size(count, index_list_size)
        + LZMA_STREAM_HEADER_SIZE as u64
}

/// 但又不能过大，以避免浪费过多的内存。
pub const INDEX_GROUP_SIZE: usize = 512;

/// 允许分配的最大记录数量
pub const PREALLOC_MAX: usize =
    (usize::MAX - std::mem::size_of::<IndexGroup>()) / std::mem::size_of::<IndexRecord>();
