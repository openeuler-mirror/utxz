/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::{LzmaAction, LzmaAllocator, LzmaRet, LZMA_TELL_ANY_CHECK};

use super::{lzma_next_end, lzma_stream_decoder_init, LzmaNextCoder};

pub fn lzma_stream_buffer_decode(
    memlimit: &mut u64,
    flags: u32,
    allocator: &LzmaAllocator,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    output: &mut Vec<u8>,
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // 检查输入输出参数的有效性
    if Some(in_pos.clone()).is_none()
        || (input.is_empty() && *in_pos != input.len())
        || *in_pos > input.len()
        || Some(out_pos.clone()).is_none()
        || (output.is_empty() && *out_pos != output.len())
        || *out_pos > output.len()
    {
        return LzmaRet::ProgError;
    }

    // 检查不允许的标志
    if flags & LZMA_TELL_ANY_CHECK != 0 {
        return LzmaRet::ProgError;
    }

    // 初始化流解码器
    let mut stream_decoder = LzmaNextCoder::default();
    let mut ret = lzma_stream_decoder_init(&mut stream_decoder, allocator, *memlimit, flags);

    if ret == LzmaRet::Ok {
        // 保存初始位置以便在出错时恢复
        let in_start = *in_pos;
        let out_start = *out_pos;

        // 执行实际解码
        if let Some(code) = stream_decoder.code {
            ret = code(
                &mut stream_decoder.coder.as_mut().unwrap(),
                allocator,
                input,
                in_pos,
                in_size,
                output,
                out_pos,
                out_size,
                LzmaAction::Finish,
            );
        }

        if ret == LzmaRet::StreamEnd {
            ret = LzmaRet::Ok;
        } else {
            // 出错时恢复位置
            *in_pos = in_start;
            *out_pos = out_start;

            if ret == LzmaRet::Ok {
                // 输入被截断或输出缓冲区太小
                assert!(*in_pos == input.len() || *out_pos == output.len());

                if *in_pos == input.len() {
                    ret = LzmaRet::DataError;
                } else {
                    ret = LzmaRet::BufError;
                }
            } else if ret == LzmaRet::MemlimitError {
                // 通知调用者需要多少内存
                let mut memusage: u64 = 0;

                if let Some(memconfig) = stream_decoder.memconfig {
                    memconfig(
                        &mut stream_decoder.coder.as_mut().unwrap(),
                        memlimit,
                        &mut memusage,
                        0,
                    );
                }
            }
        }
    }

    // 释放解码器内存
    lzma_next_end(&mut stream_decoder, allocator);

    ret
}
