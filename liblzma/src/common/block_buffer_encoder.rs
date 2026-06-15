/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_min;

use crate::{
    api::{
        LzmaAction, LzmaAllocator, LzmaBlock, LzmaFilter, LzmaOptionsLzma, LzmaOptionsType,
        LzmaRet, LZMA_CHECK_ID_MAX, LZMA_CHECK_SIZE_MAX, LZMA_DICT_SIZE_MIN, LZMA_FILTER_LZMA2,
        LZMA_VLI_BYTES_MAX, LZMA_VLI_UNKNOWN,
    },
    check::{
        lzma_check_finish, lzma_check_init, lzma_check_is_supported, lzma_check_size,
        lzma_check_update, LzmaCheckState,
    },
    common::{lzma_block_header_encode, lzma_block_header_size},
    lzma::{LZMA2_CHUNK_MAX, LZMA2_HEADER_UNCOMPRESSED},
};

use super::{lzma_next_end, lzma_raw_encoder_init, LzmaNextCoder, COMPRESSED_SIZE_MAX};

/// Maximum size of block headers, aligned to 4 bytes

pub const HEADERS_BOUND: usize = {
    let unaligned = 1 + 1 + 2 * LZMA_VLI_BYTES_MAX + 3 + 4 + LZMA_CHECK_SIZE_MAX + 3;
    unaligned & !3 // 对齐到4字节边界
};

// 计算 LZMA2 压缩的边界，返回溢出的情况
fn lzma2_bound(uncompressed_size: u64) -> u64 {
    // 防止整数溢出
    if uncompressed_size > COMPRESSED_SIZE_MAX {
        return 0;
    }

    // 计算 LZMA2 头部的确切开销：
    // 将 uncompressed_size 向上舍入到 LZMA2_CHUNK_MAX 的倍数
    // 计算每个块头的大小，并加上结束标记的一个字节
    let overhead = ((uncompressed_size + LZMA2_CHUNK_MAX as u64 - 1) / LZMA2_CHUNK_MAX as u64)
        * LZMA2_HEADER_UNCOMPRESSED as u64
        + 1;

    // 捕获可能的整数溢出
    if COMPRESSED_SIZE_MAX - overhead < uncompressed_size {
        return 0;
    }

    uncompressed_size + overhead
}

// 计算 LZMA 块缓冲区的边界
pub fn lzma_block_buffer_bound64(uncompressed_size: u64) -> u64 {
    // 如果数据没有压缩，始终使用未压缩的 LZMA2 块
    let mut lzma2_size = lzma2_bound(uncompressed_size);
    if lzma2_size == 0 {
        return 0;
    }

    // 考虑块填充
    lzma2_size = (lzma2_size + 3) & !3;

    // 因为 lzma2_bound() 已经考虑了头部大小，所以不必担心溢出
    HEADERS_BOUND as u64 + lzma2_size
}

pub fn lzma_block_buffer_bound(uncompressed_size: usize) -> usize {
    let ret = lzma_block_buffer_bound64(uncompressed_size as u64);

    ret as usize
}

fn block_encode_uncompressed(
    block: &mut LzmaBlock,
    input: &[u8],
    insize: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 创建LZMA2选项
    let mut lzma2 = LzmaOptionsLzma {
        dict_size: LZMA_DICT_SIZE_MIN,
        ..Default::default()
    };

    // 设置过滤器
    let mut filters: [LzmaFilter; 2] = [
        LzmaFilter {
            id: LZMA_FILTER_LZMA2,
            options: Some(LzmaOptionsType::LzmaOptionsLzma(lzma2)),
        },
        LzmaFilter {
            id: LZMA_VLI_UNKNOWN,
            options: None,
        },
    ];

    // 临时替换block的过滤器
    let filters_orig = std::mem::replace(&mut block.filters, filters.to_vec());
    block.filters = filters.to_vec();

    // Save and clear size fields so they're omitted from the block header
    let saved_compressed = block.compressed_size;
    let saved_uncompressed = block.uncompressed_size;
    block.compressed_size = LZMA_VLI_UNKNOWN;
    block.uncompressed_size = LZMA_VLI_UNKNOWN;

    // 编码块头部
    if lzma_block_header_size(block) != LzmaRet::Ok {
        block.compressed_size = saved_compressed;
        block.uncompressed_size = saved_uncompressed;
        block.filters = filters_orig.to_vec();
        return LzmaRet::ProgError;
    }

    let data_bound = lzma2_bound(input.len() as u64) as usize;
    if output.len() - *out_pos < block.header_size as usize + data_bound {
        block.compressed_size = saved_compressed;
        block.uncompressed_size = saved_uncompressed;
        block.filters = filters_orig.to_vec();
        return LzmaRet::BufError;
    }

    if lzma_block_header_encode(block, &mut output[*out_pos..]) != LzmaRet::Ok {
        block.compressed_size = saved_compressed;
        block.uncompressed_size = saved_uncompressed;
        block.filters = filters_orig.to_vec();
        return LzmaRet::ProgError;
    }

    block.compressed_size = saved_compressed;
    block.uncompressed_size = saved_uncompressed;
    block.filters = filters_orig.to_vec();
    *out_pos += block.header_size as usize;

    // 使用LZMA2未压缩块编码数据
    let mut in_pos = 0;
    let mut control = 0x01u8; // 字典重置

    while in_pos < input.len() {
        // 控制字节：表示未压缩块，第一个重置字典
        output[*out_pos] = control;
        *out_pos += 1;
        control = 0x02; // 不重置字典

        // 未压缩块的大小
        let copy_size = my_min(input.len() - in_pos, LZMA2_CHUNK_MAX as usize);
        output[*out_pos] = ((copy_size - 1) >> 8) as u8;
        *out_pos += 1;
        output[*out_pos] = ((copy_size - 1) & 0xFF) as u8;
        *out_pos += 1;

        // 实际数据
        assert!(*out_pos + copy_size <= output.len());
        output[*out_pos..*out_pos + copy_size].copy_from_slice(&input[in_pos..in_pos + copy_size]);

        in_pos += copy_size;
        *out_pos += copy_size;
    }

    // 结束标记
    output[*out_pos] = 0x00;
    *out_pos += 1;
    assert!(*out_pos <= output.len());

    LzmaRet::Ok
}

fn block_encode_normal(
    block: &mut LzmaBlock,
    allocator: &LzmaAllocator,
    input: &[u8],
    insize: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
    compressed_size_bound: u64,
) -> LzmaRet {
    // 获取块头部大小
    let ret: LzmaRet = lzma_block_header_size(block);
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 预留块头部空间并暂时跳过
    if output.len() - *out_pos <= block.header_size as usize {
        return LzmaRet::BufError;
    }

    let out_start = *out_pos;
    *out_pos += block.header_size as usize;

    // 限制 out_size，以便在输出超过未压缩块大小时停止编码
    let mut out_size = output.len();
    if out_size - *out_pos > compressed_size_bound as usize {
        out_size = *out_pos + compressed_size_bound as usize;
    }

    // 初始化原始编码器
    let mut raw_encoder = &mut LzmaNextCoder::default();
    let mut ret = lzma_raw_encoder_init(&mut raw_encoder, &block.filters);

    if ret == LzmaRet::Ok {
        let mut in_pos: usize = 0;
        if let Some(cpde) = raw_encoder.code {
            ret = cpde(
                &mut raw_encoder.coder.as_mut().unwrap(),
                input,
                &mut in_pos,
                insize,
                output,
                out_pos,
                out_size,
                LzmaAction::Finish,
            );
        }
    }

    // 即使 lzma_raw_encoder_init() 失败，也需要运行此代码
    lzma_next_end(raw_encoder);

    if ret == LzmaRet::StreamEnd {
        // Save compressed end position before padding
        let compressed_end = *out_pos;
        // Pad compressed data to 4-byte boundary
        while *out_pos % 4 != 0 {
            output[*out_pos] = 0x00;
            *out_pos += 1;
        }

        // Set compressed_size for Index: excludes padding and check
        block.compressed_size = (compressed_end - out_start - block.header_size as usize) as u64;

        // Write block header. We must keep compressed_size=LZMA_VLI_UNKNOWN during
        // encoding so the header omits size fields (Index provides them).
        let cs_for_header = block.compressed_size;
        block.compressed_size = LZMA_VLI_UNKNOWN;
        let header_ret = lzma_block_header_encode(block, &mut output[out_start..]);
        if header_ret != LzmaRet::Ok {
            ret = header_ret;
        }
        block.compressed_size = cs_for_header;
    } else if ret == LzmaRet::Ok {
        // 输出缓冲区已满
        ret = LzmaRet::BufError;
    }

    // 如果出现错误，重置 *out_pos
    if ret != LzmaRet::Ok && ret != LzmaRet::StreamEnd {
        *out_pos = out_start;
    }

    ret
}

fn block_buffer_encode(
    block: &mut LzmaBlock,
    allocator: &LzmaAllocator,
    input: &[u8],
    in_size: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
    try_to_compress: bool,
) -> LzmaRet {
    // 验证参数
    if Some(block.clone()).is_none()
        || Some(input).is_none() && in_size != 0
        || Some(output.clone()).is_none()
        || Some(out_pos.clone()).is_none()
        || input.is_empty()
        || *out_pos > out_size
    {
        return LzmaRet::ProgError;
    }

    if *out_pos > output.len() {
        return LzmaRet::ProgError;
    }

    // 检查版本
    if block.version > 1 {
        return LzmaRet::OptionsError;
    }

    // 验证块的内容
    if (block.check.clone() as u32) > LZMA_CHECK_ID_MAX
        || (try_to_compress && Some(block.filters.clone()).is_none())
    {
        return LzmaRet::ProgError;
    }

    if !lzma_check_is_supported(block.check.clone()) {
        return LzmaRet::UnsupportedCheck;
    }

    // 确保块大小是4的倍数
    let mut out_size = output.len();
    out_size -= (out_size - *out_pos) & 3;

    // 获取校验字段大小
    let check_size = lzma_check_size(block.check.clone());
    debug_assert!(check_size != u32::MAX as u32);

    // 为校验字段保留空间
    if out_size - *out_pos <= check_size as usize {
        return LzmaRet::BufError;
    }

    out_size -= check_size as usize;

    // 初始化 uncompressed_size 用于内部 bound 计算
    block.uncompressed_size = in_size as u64;
    block.compressed_size = lzma2_bound(in_size as u64);
    // Use LZMA_VLI_UNKNOWN for header encoding so both size fields are omitted
    // from the block header (the Index provides block boundaries)
    let compressed_size_bound = block.compressed_size;
    block.compressed_size = LZMA_VLI_UNKNOWN;
    block.uncompressed_size = LZMA_VLI_UNKNOWN;
    if compressed_size_bound == 0 {
        return LzmaRet::DataError;
    }

    // 执行实际的压缩
    let mut ret = LzmaRet::BufError;
    if try_to_compress {
        ret = block_encode_normal(
            block,
            allocator,
            input,
            in_size,
            output,
            out_pos,
            out_size,
            compressed_size_bound,
        );
    }

    if ret != LzmaRet::Ok && ret != LzmaRet::StreamEnd {
        if ret != LzmaRet::BufError {
            return ret;
        }

        let r = block_encode_uncompressed(block, input, in_size, output, out_pos, out_size);
        if r != LzmaRet::Ok {
            return r;
        }

        if block.compressed_size == LZMA_VLI_UNKNOWN {
            block.compressed_size = (*out_pos - block.header_size as usize) as u64;
        }
    }

    assert!(*out_pos <= out_size);

    // Ensure 4-byte alignment (block_encode_normal pads, but block_encode_uncompressed may not)
    while *out_pos % 4 != 0 {
        if *out_pos >= out_size {
            return LzmaRet::BufError;
        }
        output[*out_pos] = 0x00;
        *out_pos += 1;
    }

    // Handle check
    if check_size > 0 {
        let mut check = LzmaCheckState::default();
        lzma_check_init(&mut check, block.check.clone());
        lzma_check_update(&mut check, block.check.clone(), input, input.len());
        lzma_check_finish(&mut check, block.check.clone());

        block.raw_check[..check_size as usize]
            .copy_from_slice(&unsafe { check.buffer.u8 }[..check_size as usize]);
        output[*out_pos..*out_pos + check_size as usize]
            .copy_from_slice(&unsafe { check.buffer.u8 }[..check_size as usize]);
        *out_pos += check_size as usize;
    }

    LzmaRet::Ok
}

#[no_mangle]
pub fn lzma_block_buffer_encode(
    block: &mut LzmaBlock,
    allocator: &LzmaAllocator,
    input: &mut Vec<u8>,
    input_size: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 调用内部实现
    block_buffer_encode(
        block, allocator, input, input_size, output, out_pos, out_size, true,
    )
}

#[no_mangle]
pub fn lzma_block_uncomp_encode(
    block: &mut LzmaBlock,
    input: &mut Vec<u8>,
    input_size: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 调用内部实现
    block_buffer_encode(
        block,
        &LzmaAllocator::default(),
        input,
        input_size,
        output,
        out_pos,
        out_size,
        true,
    )
}
