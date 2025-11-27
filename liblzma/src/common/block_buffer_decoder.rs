/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */


use crate::api::{LzmaAction, LzmaAllocator, LzmaBlock, LzmaRet};

use super::{lzma_block_decoder_init, lzma_next_coder_init, lzma_next_end, LzmaNextCoder};

pub fn lzma_block_buffer_decode(
    block: &mut LzmaBlock,
    allocator: &LzmaAllocator,
    input: &mut Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 参数验证
    if Some(in_pos.clone()).is_none()
        || (*in_pos != in_size && input.is_empty())
        || *in_pos > in_size
        || Some(out_pos.clone()).is_none()
        || (*out_pos != out_size && output.is_empty())
        || *out_pos > out_size
    {
        return LzmaRet::ProgError;
    }

    // 初始化 Block 解码器
    let mut block_decoder: LzmaNextCoder = lzma_next_coder_init();
    let mut ret = lzma_block_decoder_init(&mut block_decoder, allocator, block);

    if ret == LzmaRet::Ok {
        // 记录初始位置，以便在错误时恢复
        let in_start = *in_pos;
        let out_start = *out_pos;

        // 执行实际解码
        if let Some(code) = block_decoder.code {
            ret = code(
                block_decoder.coder.as_mut().unwrap(),
                allocator,
                input,
                in_pos,
                in_size,
                output,
                out_pos,
                out_size,
                LzmaAction::Finish,
            )
        }

        if ret == LzmaRet::StreamEnd {
            ret = LzmaRet::Ok;
        } else {
            if ret == LzmaRet::Ok {
                // 输入被截断或输出缓冲区太小
                assert!(*in_pos == in_size || *out_pos == out_size);

                // 检测是否是输入截断还是输出缓冲区溢出
                if *in_pos == in_size {
                    ret = LzmaRet::DataError;
                } else {
                    ret = LzmaRet::BufError;
                }
            }

            // 恢复输入/输出位置
            *in_pos = in_start;
            *out_pos = out_start;
        }
    }

    // 释放解码器内存
    lzma_next_end(&mut block_decoder, allocator);

    ret
}
