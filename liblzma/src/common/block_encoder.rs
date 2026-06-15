/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::any::Any;

use crate::{
    api::{
        LzmaAction, LzmaBlock, LzmaCheck, LzmaFilter, LzmaRet, LzmaStream, LzmaVli,
        LZMA_CHECK_ID_MAX, LZMA_CHECK_SIZE_MAX, LZMA_VLI_MAX,
    },
    check::{
        lzma_check_finish, lzma_check_init, lzma_check_is_supported, lzma_check_size,
        lzma_check_update, LzmaCheckState,
    },
    common::NextCoderInitFunction,
};

use super::{
    lzma_block_decoder_init, lzma_bufcpy, lzma_end, lzma_next_coder_init, lzma_next_end,
    lzma_next_filter_update, lzma_raw_encoder_init, lzma_strm_init, CoderType, LzmaNextCoder,
};

// const LZMA_VLI_MAX: u64 = 0xFFFFFFFFFFFFFFFF; // 假设最大值
// const LZMA_BLOCK_HEADER_SIZE_MAX: u64 = 1024; // 需要根据实际情况调整
// const LZMA_CHECK_SIZE_MAX: u64 = 64; // 需要根据实际情况调整

/// 计算 `COMPRESSED_SIZE_MAX` 常量
pub const COMPRESSED_SIZE_MAX: u64 = (LZMA_VLI_MAX - 1024 - LZMA_CHECK_SIZE_MAX as u64) & !3;

#[derive(Debug)]
pub struct LzmaBlockEncoder {
    /// 过滤器链，由 `lzma_raw_decoder_init()` 初始化
    pub next: Box<LzmaNextCoder>,

    /// 编码选项；当编码完成后，我们会将 Unpadded Size、Compressed Size 和 Uncompressed Size
    /// 写回到这个结构体中。
    pub block: Option<LzmaBlock>,

    /// 当前的编码阶段
    pub sequence: Sequence,

    /// 编码过程中计算出的压缩大小
    pub compressed_size: LzmaVli,

    /// 编码过程中计算出的未压缩大小
    pub uncompressed_size: LzmaVli,

    /// 在校验字段（Check field）中的位置
    pub pos: usize,

    /// 未压缩数据的校验值
    check: LzmaCheckState,
}
impl LzmaBlockEncoder {
    pub fn new() -> Self {
        LzmaBlockEncoder {
            next: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            block: None,                              // 初始化为 None
            sequence: Sequence::Code,                 // 假设 Sequence 实现了 Default
            compressed_size: LzmaVli::default(),      // 假设 LzmaVli 实现了 Default
            uncompressed_size: LzmaVli::default(),    // 同上
            pos: 0,                                   // 初始化为 0
            check: LzmaCheckState::default(),         // 假设 LzmaCheckState 实现了 Default
        }
    }

    /// 获取块的实际大小信息
    pub fn get_block_info(&self) -> Option<LzmaBlock> {
        self.block.as_ref().cloned()
    }
}

/// 编码的不同阶段
#[derive(Debug, PartialEq, Eq)]
enum Sequence {
    Code,
    Padding,
    Check,
}

fn block_encode(
    coder_ptr: &mut CoderType, // 原 C 代码中的 void* 指针
    input: &[u8],              // 输入缓冲区
    in_pos: &mut usize,        // 输入数据当前位置
    in_size: usize,            // 输入数据总大小
    output: &mut [u8],         // 输出缓冲区
    out_pos: &mut usize,       // 输出数据当前位置
    out_size: usize,           // 输出数据总大小
    action: LzmaAction,        // 当前编码操作
) -> LzmaRet {
    // println!("============ block_encode");
    // 尝试将 coder_ptr 转换为具体的 lzma_block_coder 类型
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::BlockEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    // 确保未压缩数据大小不会溢出
    if LzmaVli::MAX - coder.uncompressed_size < in_size.saturating_sub(*in_pos) as u64 {
        return LzmaRet::DataError;
    }
    // println!("==== coder  {:#?}", coder);
    loop {
        match coder.sequence {
            Sequence::Code => {
                let in_start = *in_pos;
                let out_start = *out_pos;

                let mut ret: LzmaRet = LzmaRet::Ok;

                if let Some(code) = coder.next.code {
                    ret = code(
                        coder.next.coder.as_mut().unwrap(),
                        input,
                        in_pos,
                        in_size,
                        output,
                        out_pos,
                        out_size,
                        action.clone(),
                    );
                }

                let in_used = *in_pos - in_start;
                let out_used = *out_pos - out_start;

                // 确保压缩数据大小不会超出允许范围
                if COMPRESSED_SIZE_MAX - coder.compressed_size < out_used as u64 {
                    return LzmaRet::DataError;
                }

                coder.compressed_size += out_used as u64;
                coder.uncompressed_size += in_used as u64;

                // 仅在输入数据被消耗时更新校验值，避免未定义行为
                if in_used > 0 {
                    if let Some(block) = coder.block.as_ref() {
                        lzma_check_update(
                            &mut coder.check,
                            block.check.clone(),
                            &input[in_start..in_start + in_used],
                            in_used,
                        );
                    }
                }

                // 如果流未结束或者执行的是同步刷新操作，则直接返回
                if ret != LzmaRet::StreamEnd || action == LzmaAction::SyncFlush {
                    return ret;
                }

                debug_assert!(*in_pos == in_size);
                debug_assert!(action == LzmaAction::Finish);

                // 记录压缩和未压缩数据的最终大小
                if let Some(block) = coder.block.as_mut() {
                    block.compressed_size = coder.compressed_size;
                    block.uncompressed_size = coder.uncompressed_size;
                }

                // 进入填充阶段
                coder.sequence = Sequence::Padding;
                continue;
            }

            Sequence::Padding => {
                // 填充压缩数据，使其对齐到 4 字节
                while coder.compressed_size & 3 != 0 {
                    if *out_pos >= out_size {
                        return LzmaRet::Ok;
                    }

                    output[*out_pos] = 0x00;
                    *out_pos += 1;
                    coder.compressed_size += 1;
                }

                // 如果不需要校验值，直接返回流结束
                if let Some(block) = coder.block.as_ref() {
                    if block.check == LzmaCheck::None {
                        return LzmaRet::StreamEnd;
                    }
                }

                // 完成数据校验
                if let Some(block) = coder.block.as_ref() {
                    lzma_check_finish(&mut coder.check, block.check.clone());
                }

                // 进入校验阶段
                coder.sequence = Sequence::Check;
                continue;
            }

            Sequence::Check => {
                if let Some(block) = coder.block.as_ref() {
                    let check_size = lzma_check_size(block.check.clone());

                    // 复制校验数据到输出缓冲区
                    // 在这个地方，先将coder.check.buffer.u64中的数据转移保存到u8中
                    for i in 0..8 {
                        let bytes_ne = &mut coder.check.buffer.u64[i].to_ne_bytes();
                        &mut coder.check.buffer.u8[i * 8..(i + 1) * 8].copy_from_slice(bytes_ne);
                    }
                    lzma_bufcpy(
                        &mut coder.check.buffer.u8[..check_size as usize],
                        &mut coder.pos,
                        check_size as usize,
                        output,
                        out_pos,
                        out_size,
                    );

                    // 如果校验数据未完全写入，则返回继续处理
                    if coder.pos < check_size as usize {
                        return LzmaRet::Ok;
                    }

                    // 复制最终的校验值到 block 结构中
                    if let Some(block) = coder.block.as_mut() {
                        block.raw_check[..check_size as usize].copy_from_slice(
                            &unsafe { coder.check.buffer.u8 }[..check_size as usize],
                        );
                    }
                    // println!("========== kkkkkkkkkkkkkkk");
                    return LzmaRet::StreamEnd;
                }
            }
        }
    }
    LzmaRet::ProgError
}

fn block_encoder_end(coder_ptr: &mut CoderType) {
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::AutoDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.next);
}

// Rust 版本的 lzma_block_encoder_update
fn block_encoder_update(
    coder_ptr: &mut CoderType, //

    filters: Option<&[LzmaFilter]>,  // 未使用的 filters 参数
    reversed_filters: &[LzmaFilter], // 倒序过滤器链
) -> LzmaRet {
    // 类型转换
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::BlockEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    // 如果当前状态不是 SEQ_CODE，则返回错误
    if coder.sequence != Sequence::Code {
        return LzmaRet::ProgError;
    }

    // 调用下一级过滤器链的 update 方法
    lzma_next_filter_update(&mut coder.next, reversed_filters)
}

// Rust 版本的 lzma_block_encoder_init
pub fn lzma_block_encoder_init(next: &mut LzmaNextCoder, block: &LzmaBlock) -> LzmaRet {
    // 初始化 next->coder
    if next.init != Some(NextCoderInitFunction::BlockEncoder(lzma_block_encoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::BlockEncoder(lzma_block_encoder_init));

    if Some(block).is_none() {
        return LzmaRet::ProgError;
    }

    // 检查 block 是否为 NULL
    if block.version > 1 {
        return LzmaRet::OptionsError;
    }

    // 检查 Check ID 是否超出最大值
    if (block.check.clone() as u32) > LZMA_CHECK_ID_MAX {
        return LzmaRet::ProgError;
    }

    // 检查是否支持指定的 Check 类型
    if !lzma_check_is_supported(block.check.clone()) {
        return LzmaRet::UnsupportedCheck;
    }

    // 为 next->coder 分配内存（通过 Option 管理动态内存
    let mut coder: &mut LzmaBlockEncoder = &mut LzmaBlockEncoder::new();
    if next.coder.is_none() {
        let mut new_coder = LzmaBlockEncoder {
            next: Box::new(LzmaNextCoder::default()),
            block: None,
            sequence: Sequence::Code,
            compressed_size: 0,
            uncompressed_size: 0,
            pos: 0,
            check: LzmaCheckState::default(),
        };

        next.coder = Some(CoderType::BlockEncoder(new_coder));
        next.code = Some(block_encode);
        next.end = Some(block_encoder_end);
        next.update = Some(block_encoder_update);
        coder.next = Box::new(LzmaNextCoder::default());
    }

    coder = match next.coder {
        Some(CoderType::BlockEncoder(ref mut c)) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.sequence = Sequence::Code;
    coder.block = Some(block.clone());
    coder.compressed_size = 0;
    coder.uncompressed_size = 0;
    coder.pos = 0;

    lzma_check_init(&mut coder.check, block.check.clone());
    // 初始化过滤器链
    lzma_raw_encoder_init(&mut coder.next, &block.filters)
}

/// 初始化 LZMA 块编码器并设置流支持的操作
///
/// 该函数负责：
/// 1. 初始化 LZMA 流 (`lzma_strm_init`)
/// 2. 初始化块编码器 (`lzma_block_encoder_init`)
/// 3. 设置流支持的操作 (Run/SyncFlush/Finish)
///
/// # 参数
/// * `stream` - 要初始化的 LZMA 流
/// * `block` - 包含压缩设置的 LZMA 块
///
/// # 返回值
/// 返回操作结果状态码 (`LzmaRet::Ok` 表示成功)
// Rust 版本的 lzma_block_encoder
fn lzma_block_encoder<'a>(stream: &mut LzmaStream<'a>, block: &'a LzmaBlock) -> LzmaRet {
    // 初始化流
    // lzma_next_strm_init(lzma_block_encoder_init, stream, block);
    let ret: LzmaRet = lzma_strm_init(Some(stream));
    if ret != LzmaRet::Ok {
        return ret;
    }

    let ret: LzmaRet = lzma_block_encoder_init(
        &mut stream
            .internal
            .borrow_mut()
            .as_mut()
            .unwrap()
            .next
            .as_mut()
            .unwrap(),
        block,
    );
    if ret != LzmaRet::Ok {
        lzma_end(Some(stream));
        return ret;
    }

    // 设置支持的编码动作
    stream
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .supported_actions[LzmaAction::Run as usize] = true;
    stream
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .supported_actions[LzmaAction::SyncFlush as usize] = true;
    stream
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .supported_actions[LzmaAction::Finish as usize] = true;

    LzmaRet::Ok
}
