/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_min;

use crate::{
    api::{
        LzmaAllocator, LzmaBlock, LzmaCheck, LzmaFilter, LzmaRet, LzmaStreamFlags,
        LZMA_CHECK_ID_MAX, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_BYTES_MAX, LZMA_VLI_MAX,
    },
    check::lzma_check_is_supported,
};

use super::{
    lzma_block_buffer_bound, lzma_block_buffer_encode, lzma_block_unpadded_size, lzma_index_append,
    lzma_index_buffer_encode, lzma_index_end, lzma_index_init, lzma_index_size,
    lzma_stream_footer_encode, lzma_stream_header_encode,
};

/// 具有一个记录的索引的最大大小。
/// 索引指示符 + 记录数 + 记录 + CRC32，四舍五入到下一个四的倍数。
pub const INDEX_BOUND: usize = ((1 + 1 + 2 * LZMA_VLI_BYTES_MAX + 4 + 3) & !3);

/// 流头、流尾和索引的大小。
pub const HEADERS_BOUND: usize = 2 * LZMA_STREAM_HEADER_SIZE + INDEX_BOUND;

/// 计算流缓冲区的最大边界。
pub fn lzma_stream_buffer_bound(uncompressed_size: usize) -> usize {
    // 获取块可能的最大大小。
    let block_bound = lzma_block_buffer_bound(uncompressed_size);
    if block_bound == 0 {
        return 0;
    }

    // 捕获可能的整数溢出，并防止流的大小超过 LZMA_VLI_MAX（在 64 位系统上理论上可能发生）。
    if my_min(usize::MAX, LZMA_VLI_MAX as usize) - block_bound < HEADERS_BOUND {
        return 0;
    }

    block_bound + HEADERS_BOUND
}

/// 编码流缓冲区。
pub fn lzma_stream_buffer_encode(
    filters: &[LzmaFilter],
    check: LzmaCheck,
    allocator: &LzmaAllocator,
    input: &mut Vec<u8>,
    in_size: usize,
    output: &mut Vec<u8>,
    out_pos_ptr: &mut usize,
    mut out_size: usize,
) -> LzmaRet {
    // 参数校验
    if filters.is_empty()
        || check.clone() as u32 > LZMA_CHECK_ID_MAX
        || (input.is_empty() && !input.is_empty())
        || output.is_empty()
        || *out_pos_ptr > output.len()
    {
        return LzmaRet::ProgError;
    }

    if !lzma_check_is_supported(check.clone()) {
        return LzmaRet::UnsupportedCheck;
    }

    // 使用本地副本。只有在一切成功的情况下，我们才更新 *out_pos_ptr。
    let mut out_pos = *out_pos_ptr;

    // 检查是否有足够的空间容纳流头和流尾。
    if out_size - out_pos <= 2 * LZMA_STREAM_HEADER_SIZE {
        return LzmaRet::BufError;
    }

    // 保留流尾的空间，以便在编码流尾之前不需要再次检查可用空间。
    out_size = out_size - LZMA_STREAM_HEADER_SIZE;

    // 编码流头。
    let tmp = check.clone();
    let mut stream_flags = LzmaStreamFlags {
        version: 0,
        tmp,
        ..Default::default()
    };

    if lzma_stream_header_encode(&stream_flags, &mut output[out_pos..]) != LzmaRet::Ok {
        return LzmaRet::ProgError;
    }

    out_pos += LZMA_STREAM_HEADER_SIZE;

    // 如果有输入字节，才编码块
    let tmp = check.clone();
    let mut block = LzmaBlock {
        version: 0,
        check: tmp,
        filters: filters.to_vec(),
        ..Default::default()
    };

    if !input.is_empty() {
        let ret = lzma_block_buffer_encode(
            &mut block,
            allocator,
            input,
            in_size,
            output,
            &mut out_pos,
            out_size,
        );
        if ret != LzmaRet::Ok {
            return ret;
        }
    }

    // 编码索引
    {
        // 创建一个索引。如果有输入字节，索引将包含一个记录，否则索引为空。
        let mut i = match lzma_index_init(&allocator) {
            Some(idx) => idx,
            None => return LzmaRet::MemError,
        };

        let mut ret = LzmaRet::Ok;

        if !input.is_empty() {
            ret = lzma_index_append(
                &mut i,
                allocator,
                lzma_block_unpadded_size(&block),
                block.uncompressed_size,
            );
        }

        // 如果添加记录成功，编码索引，并获取其大小，存储到流尾。
        if ret == LzmaRet::Ok {
            ret = lzma_index_buffer_encode(&i, output, &mut out_pos, out_size);

            stream_flags.backward_size = lzma_index_size(&i);
        }

        lzma_index_end(&mut i, allocator);

        if ret != LzmaRet::Ok {
            return ret;
        }
    }

    // 编码流尾。我们已经为此预留了空间。
    if lzma_stream_footer_encode(&mut stream_flags, &mut output[out_pos..]) != LzmaRet::Ok {
        return LzmaRet::ProgError;
    }

    out_pos += LZMA_STREAM_HEADER_SIZE;

    // 一切正常，将新的输出位置提供给应用程序。
    *out_pos_ptr = out_pos;
    LzmaRet::Ok
}
