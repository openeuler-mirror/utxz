/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::cmp::min;

use crate::{
    api::{LzmaAction, LzmaFilter, LzmaRet},
    common::{
        lzma_bufcpy, lzma_next_end, lzma_next_filter_init, lzma_next_filter_update, CoderType,
        LzmaFilterInfo, LzmaNextCoder,
    },
};

use super::{LzmaSimpleCoder, LzmaSimpleX86, SimpleType};


/// 复制或编码/解码更多数据到out[]
#[allow(clippy::too_many_arguments)]
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
                action,
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

/// 调用过滤器
fn call_filter(coder: &mut LzmaSimpleCoder, buffer: &mut [u8], size: usize) -> usize {
    let filtered = (coder.filter.unwrap())(
        &mut coder.simple.clone(),
        coder.now_pos,
        coder.is_encoder,
        buffer,
        size,
    );
    coder.now_pos += filtered as u32;
    filtered
}

/// 简单编码实现
#[allow(clippy::too_many_arguments)]
fn simple_code(
    coder_ptr: &mut CoderType,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::SimpleCoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // TODO: 添加对LZMA_SYNC_FLUSH的部分支持
    if action == LzmaAction::SyncFlush {
        return LzmaRet::OptionsError;
    }

    // 从coder.buffer[]刷新已过滤的数据到out[]
    if coder.pos < coder.filtered {
        lzma_bufcpy(
            &coder.buffer,
            &mut coder.pos,
            coder.filtered,
            output,
            out_pos,
            out_size,
        );

        if coder.pos < coder.filtered {
            return LzmaRet::Ok;
        }

        if coder.end_was_reached {
            debug_assert!(coder.filtered == coder.size);
            return LzmaRet::StreamEnd;
        }
    }

    // 如果到达这里，缓冲区中没有已过滤的数据
    coder.filtered = 0;
    debug_assert!(!coder.end_was_reached);

    // 处理输出空间和未过滤数据
    let out_avail = out_size - *out_pos;
    let buf_avail = coder.size - coder.pos;

    if out_avail > buf_avail || buf_avail == 0 {
        let out_start = *out_pos;

        // 刷新数据但不重置位置
        if buf_avail > 0 {
            output[*out_pos..*out_pos + buf_avail]
                .copy_from_slice(&coder.buffer[coder.pos..coder.pos + buf_avail]);
            *out_pos += buf_avail;
        }

        // 复制/编码/解码更多数据到out[]
        let ret = copy_or_code(
            coder, input, in_pos, in_size, output, out_pos, out_size, action,
        );
        debug_assert!(ret != LzmaRet::StreamEnd);
        if ret != LzmaRet::Ok {
            return ret;
        }

        // 过滤输出
        let size = *out_pos - out_start;
        let filtered = if size == 0 {
            0
        } else {
            call_filter(coder, &mut output[out_start..], size)
        };

        let unfiltered = size - filtered;
        debug_assert!(unfiltered <= coder.allocated / 2);

        // 更新位置和大小
        coder.pos = 0;
        coder.size = unfiltered;

        if coder.end_was_reached {
            coder.size = 0;
        } else if unfiltered > 0 {
            *out_pos -= unfiltered;
            coder.buffer[..unfiltered].copy_from_slice(&output[*out_pos..*out_pos + unfiltered]);
        }
    } else if coder.pos > 0 {
        // 使用Rust的内存移动
        coder
            .buffer
            .copy_within(coder.pos..coder.pos + buf_avail, 0);
        coder.size -= coder.pos;
        coder.pos = 0;
    }

    debug_assert!(coder.pos == 0);

    // 处理非空缓冲区
    let mut output = coder.buffer.clone();
    if coder.size > 0 {
        let mut out_pos_local = coder.size;
        let ret = copy_or_code(
            coder,
            input,
            in_pos,
            in_size,
            output.as_mut(),
            &mut out_pos_local,
            coder.allocated,
            action, // 如果 action 已被 move，应重新 clone 或使用引用
        );
        debug_assert!(ret != LzmaRet::StreamEnd);
        if ret != LzmaRet::Ok {
            return ret;
        }

        let mut coder_buffer = coder.buffer.clone();
        coder.filtered = call_filter(coder, &mut coder_buffer, coder.size);

        if coder.end_was_reached {
            coder.filtered = coder.size;
        }

        // 尽可能多地刷新
        let copy_size = min(coder.filtered - coder.pos, out_size - *out_pos);
        if copy_size > 0 {
            output[*out_pos..*out_pos + copy_size]
                .copy_from_slice(&coder.buffer[coder.pos..coder.pos + copy_size]);
        }
        coder.pos += copy_size;
        *out_pos += copy_size;
    }

    // 检查是否完成所有工作
    if coder.end_was_reached && coder.pos == coder.size {
        return LzmaRet::StreamEnd;
    }

    LzmaRet::Ok
}

type FilterFn = fn(&mut SimpleType, u32, bool, &mut [u8], usize) -> usize;
/// 结束简单编码器
fn simple_coder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::SimpleCoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(coder.next.as_mut())
    // Rust的内存管理会自动处理释放
}

/// 更新简单编码器
fn simple_coder_update(
    coder_ptr: &mut CoderType,

    _filters_null: Option<&[LzmaFilter]>,
    reversed_filters: &[LzmaFilter],
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::SimpleCoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    // 没有更新支持，只调用链中的下一个过滤器
    lzma_next_filter_update(&mut coder.next, &reversed_filters[1..])
}

/// 初始化简单编码器
#[allow(unused_assignments)]
pub fn lzma_simple_coder_init(
    next: &mut LzmaNextCoder,

    filters: &[LzmaFilterInfo],
    filter: FilterFn,
    simple_size: usize,
    unfiltered_max: usize,
    alignment: u32,
    is_encoder: bool,
) -> LzmaRet {
    let mut coder: &mut LzmaSimpleCoder = &mut LzmaSimpleCoder::new(0);
    // 使用Rust的Option来处理可能为空的编码器
    if next.coder.is_none() {
        // 创建新的编码器
        let mut coder_ = LzmaSimpleCoder::new(unfiltered_max * 2);
        coder_.filter = Some(filter);
        coder_.allocated = 2 * unfiltered_max;
        if simple_size > 0 {
            coder_.simple = SimpleType::X86Filter(LzmaSimpleX86::default());
        }
        // coder = &mut coder_;
        next.code = Some(simple_code);
        next.end = Some(simple_coder_end);
        next.update = Some(simple_coder_update);
        next.coder = Some(CoderType::SimpleCoder(coder_));

        if let Some(CoderType::SimpleCoder(ref mut c)) = next.coder {
            coder = c;
        } else {
            return LzmaRet::ProgError;
        }
    } else {
        coder = match next.coder.as_mut() {
            Some(CoderType::SimpleCoder(ref mut c)) => c,
            _ => unreachable!(),
        };
    }

    // 设置起始偏移
    if let Some(options) = &filters[0].options {
        let simple = options.as_bcj().unwrap();
        coder.now_pos = simple.start_offset;
        if coder.now_pos & (alignment - 1) != 0 {
            return LzmaRet::OptionsError;
        }
    } else {
        coder.now_pos = 0;
    }

    // 重置变量
    coder.is_encoder = is_encoder;
    coder.end_was_reached = false;
    coder.pos = 0;
    coder.filtered = 0;
    coder.size = 0;

    // 初始化下一个过滤器
    lzma_next_filter_init(&mut coder.next, &filters[1..])
}
