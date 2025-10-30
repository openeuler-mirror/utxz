/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

 
 use crate::{
     api::{LzmaAction, LzmaRet},
     common::{
         lzma_bufcpy, 
         LzmaFilterInfo, LzmaNextCoder,
     },
 };
 
 use super::{LzmaSimpleCoder,   SimpleType};



/// 复制或编码/解码更多数据到out[]
fn copy_or_code(
    coder: &mut LzmaSimpleCoder,

    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    debug_assert!(!coder.end_was_reached);

    if coder.next.code.is_none() {
        // 使用Rust的内存复制
        lzma_bufcpy(input, in_pos, in_size, output, out_pos, out_size);
        // let copy_size = min(
        //     in_size - *in_pos,
        //     out_size - *out_pos
        // );
        // output[*out_pos..*out_pos + copy_size]
        //     .copy_from_slice(&input[*in_pos..*in_pos + copy_size]);
        // *in_pos += copy_size;
        // *out_pos += copy_size;

        // 检查是否到达流末尾
        if coder.is_encoder && action == LzmaAction::Finish && *in_pos == in_size {
            coder.end_was_reached = true;
        }
    } else {
        // 调用链中的下一个编码器以提供数据
        let ret = match &coder.next.code {
            Some(code) => code(
                coder.next.coder.as_mut().unwrap(),
                input,
                in_pos,
                in_size,
                output,
                out_pos,
                out_size,
                action.clone(),
            ),
            None => LzmaRet::Ok,
        };

        match ret {
            LzmaRet::StreamEnd => {
                debug_assert!(!coder.is_encoder || action == LzmaAction::Finish);
                coder.end_was_reached = true;
            }
            LzmaRet::Ok => {}
            _ => return ret,
        }
    }

    LzmaRet::Ok
}


type FilterFn = fn(&mut SimpleType, u32, bool, &mut [u8], usize) -> usize;
/// 初始化简单编码器
pub fn lzma_simple_coder_init(
    _next: &mut LzmaNextCoder,
    _filters: &[LzmaFilterInfo],
    _filter: FilterFn,
    _simple_size: usize,
    _unfiltered_max: usize,
    _alignment: u32,
    _is_encoder: bool,
) -> LzmaRet {
    LzmaRet::Ok
}
