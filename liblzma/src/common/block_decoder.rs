/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::common::NextCoderInitFunction;
use crate::{LZMA_VLI_MAX, LZMA_VLI_UNKNOWN};
use common::my_min;

use crate::{
    api::{lzma_vli_is_valid, LzmaAction, LzmaBlock, LzmaCheck, LzmaRet, LzmaStream, LzmaVli},
    check::{
        lzma_check_finish, lzma_check_init, lzma_check_is_supported, lzma_check_size,
        lzma_check_update, LzmaCheckState,
    },
    common::lzma_block_unpadded_size,
    LZMA_VLI_C,
};

use super::{
    lzma_bufcpy, lzma_end, lzma_index_encoder_init, lzma_next_end, lzma_raw_decoder_init,
    lzma_strm_init, CoderType, LzmaNextCoder,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sequence {
    #[default]
    Code,
    Padding,
    Check,
}

#[derive(Debug)]
pub struct LzmaBlockDecoder {
    /// 解码序列状态
    sequence: Sequence,

    /// 解码链中的过滤器
    next: Box<LzmaNextCoder>,

    /// 解码选项；解码完成后将压缩大小和未压缩大小写回此结构
    block: Option<Box<LzmaBlock>>,

    /// 解码时计算的压缩大小
    compressed_size: LzmaVli,

    /// 解码时计算的未压缩大小
    uncompressed_size: LzmaVli,

    /// 最大允许的压缩大小；考虑了块头和校验字段的大小
    compressed_limit: LzmaVli,

    /// 最大允许的未压缩大小
    uncompressed_limit: LzmaVli,

    /// 读取校验字段时的位置
    check_pos: usize,

    /// 未压缩数据的校验
    check: LzmaCheckState,

    /// 如果完整性校验不被计算和验证，则为 true
    ignore_check: bool,
}

#[inline]
fn is_size_valid(size: u64, reference: u64) -> bool {
    reference == LZMA_VLI_UNKNOWN || reference == size
}

fn block_decode(
    coder_ptr: &mut CoderType,

    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::BlockDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    loop {
        match coder.sequence {
            Sequence::Code => {
                let in_start = *in_pos;
                let out_start = *out_pos;

                // 计算输入/输出限制
                let in_stop = *in_pos
                    + my_min(
                        in_size - *in_pos,
                        (coder.compressed_limit - coder.compressed_size) as usize,
                    );

                let out_stop = *out_pos
                    + my_min(
                        out_size - *out_pos,
                        (coder.uncompressed_limit - coder.uncompressed_size) as usize,
                    );

                // 调用下一级解码器进行解码
                let mut ret = LzmaRet::Ok;
                if let Some(code) = coder.next.code {
                    ret = code(
                        coder.next.coder.as_mut().unwrap(),
                        input,
                        in_pos,
                        in_stop,
                        output,
                        out_pos,
                        out_stop,
                        action,
                    );
                }

                let in_used = *in_pos - in_start;
                let out_used = *out_pos - out_start;

                // 更新压缩/未压缩大小
                coder.compressed_size += in_used as u64;
                coder.uncompressed_size += out_used as u64;

                if ret == LzmaRet::Ok {
                    let (declared_compressed_size, declared_uncompressed_size) = {
                        let block = coder.block.as_mut().unwrap();
                        (block.compressed_size, block.uncompressed_size)
                    };
                    let comp_done = coder.compressed_size == declared_compressed_size;
                    let uncomp_done = coder.uncompressed_size == declared_uncompressed_size;

                    if comp_done && uncomp_done {
                        return LzmaRet::DataError;
                    }

                    if comp_done && *out_pos < out_size {
                        return LzmaRet::DataError;
                    }

                    if uncomp_done && *in_pos < in_size {
                        return LzmaRet::DataError;
                    }
                }

                // 更新完整性校验
                if !coder.ignore_check && out_used > 0 {
                    lzma_check_update(
                        &mut coder.check,
                        coder.block.as_mut().unwrap().check.clone(),
                        &output[out_start..out_start + out_used],
                        out_used as usize,
                    );
                }

                if ret != LzmaRet::StreamEnd {
                    return ret;
                }

                // 验证最终的压缩/未压缩大小
                let (declared_compressed_size, declared_uncompressed_size) = {
                    let block = coder.block.as_mut().unwrap();
                    (block.compressed_size, block.uncompressed_size)
                };
                if !is_size_valid(coder.compressed_size, declared_compressed_size)
                    || !is_size_valid(coder.uncompressed_size, declared_uncompressed_size)
                {
                    return LzmaRet::DataError;
                }

                // 记录最终大小
                coder.block.as_mut().unwrap().compressed_size = coder.compressed_size;
                coder.block.as_mut().unwrap().uncompressed_size = coder.uncompressed_size;

                coder.sequence = Sequence::Padding;
                continue;
            }

            // 处理 Padding 阶段
            Sequence::Padding => {
                while coder.compressed_size & 3 != 0 {
                    if *in_pos >= in_size {
                        return LzmaRet::Ok;
                    }

                    coder.compressed_size += 1;

                    if input[*in_pos] != 0x00 {
                        return LzmaRet::DataError;
                    }

                    *in_pos += 1;
                }

                if coder.block.as_mut().unwrap().check == LzmaCheck::None {
                    return LzmaRet::StreamEnd;
                }

                if !coder.ignore_check {
                    lzma_check_finish(
                        &mut coder.check,
                        coder.block.as_mut().unwrap().check.clone(),
                    );
                }

                coder.sequence = Sequence::Check;
                continue;
            }

            // 处理 Check 阶段
            Sequence::Check => {
                let check_size = lzma_check_size(coder.block.as_mut().unwrap().check.clone());
                {
                    let block = coder.block.as_mut().unwrap();
                    lzma_bufcpy(
                        input,
                        in_pos,
                        in_size,
                        &mut block.raw_check[..check_size as usize],
                        &mut coder.check_pos,
                        check_size as usize,
                    );
                }

                if coder.check_pos < check_size as usize {
                    return LzmaRet::Ok;
                }

                if !coder.ignore_check
                    && lzma_check_is_supported(coder.block.as_mut().unwrap().check.clone())
                    && coder.block.as_mut().unwrap().raw_check[..check_size as usize]
                        != unsafe { coder.check.buffer.u8 }[..check_size as usize]
                {
                    return LzmaRet::DataError;
                }

                return LzmaRet::StreamEnd;
            }
            _ => {
                break;
            }
        }
    }

    LzmaRet::ProgError
}

fn block_decoder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::AutoDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.next);
}

pub fn lzma_block_decoder_init(next: &mut LzmaNextCoder, block: &mut LzmaBlock) -> LzmaRet {
    // 初始化 next->coder
    // lzma_next_coder_init(&lzma_block_decoder_init, next, allocator);
    if next.init != Some(NextCoderInitFunction::BlockDecoder(lzma_block_decoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::BlockDecoder(lzma_block_decoder_init));

    // 验证选项：lzma_block_unpadded_size 函数已经做了大部分验证
    // 但我们还需要验证 Uncompressed Size 和 filters。
    if lzma_block_unpadded_size(block) == 0 || !lzma_vli_is_valid(block.uncompressed_size) {
        return LzmaRet::ProgError;
    }

    // 如果需要，分配 next->coder
    if next.coder.is_none() {
        let new_coder = LzmaBlockDecoder {
            sequence: Sequence::Code,
            next: Box::new(LzmaNextCoder::default()),
            block: None,
            compressed_size: 0,
            uncompressed_size: 0,
            compressed_limit: 0,
            uncompressed_limit: 0,
            check_pos: 0,
            check: LzmaCheckState::default(),
            ignore_check: false,
        };
        next.code = Some(block_decode);
        next.end = Some(block_decoder_end);
        next.coder = Some(CoderType::BlockDecoder(new_coder));
    }

    let coder = match &mut next.coder {
        Some(CoderType::BlockDecoder(c)) => c,
        _ => return LzmaRet::ProgError,
    };

    // 基本初始化
    coder.sequence = Sequence::Code;
    coder.block = Some(Box::new(block.clone()));
    coder.compressed_size = 0;
    coder.uncompressed_size = 0;
    //    coder.next = Box::new(LzmaNextCoder::default());

    // 如果压缩大小未知，则计算最大允许值，使得块的编码大小（包括块填充）仍然是有效的 VLI 并且是 4 的倍数
    coder.compressed_limit = if block.compressed_size == LZMA_VLI_UNKNOWN {
        ((LZMA_VLI_MAX & !LZMA_VLI_C!(3) as u64)
            - block.header_size as u64
            - lzma_check_size(block.check.clone()) as u64)
            .into()
    } else {
        block.compressed_size
    };

    // 对于未压缩大小，如果块头缺少大小信息，则 LZMA_VLI_MAX 是最大的可能未压缩大小
    coder.uncompressed_limit = if block.uncompressed_size == LZMA_VLI_UNKNOWN {
        LZMA_VLI_MAX as u64
    } else {
        block.uncompressed_size
    };

    // 初始化校验
    coder.check_pos = 0;
    lzma_check_init(&mut coder.check, block.check.clone());

    // 设置是否忽略校验
    coder.ignore_check = if block.version >= 1 {
        block.ignore_check
    } else {
        false
    };

    // 初始化滤波链
    lzma_raw_decoder_init(&mut coder.next, &block.filters);

    // 在解码完成后，将结果写回原始的 block 参数
    // 这里我们需要在 block_decode 函数中实现这个功能
    // 暂时先返回成功
    LzmaRet::Ok
}

pub fn lzma_block_decoder<'a>(strm: &mut LzmaStream<'a>, block: &'a mut LzmaBlock) -> LzmaRet {
    // 初始化流的解码器
    let ret: LzmaRet = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    let mut internal = strm.internal.borrow_mut();
    if let Some(ref mut internal) = *internal {
        if let Some(ref mut next) = internal.next {
            let ret: LzmaRet = lzma_block_decoder_init(next, block);
            if ret != LzmaRet::Ok {
                let _ = internal;
                // lzma_end(Some(strm));
                return ret;
            }

            // 启用支持的操作
            internal.supported_actions[LzmaAction::Run as usize] = true;
            internal.supported_actions[LzmaAction::Finish as usize] = true;

            LzmaRet::Ok
        } else {
            let _ = internal;
            // lzma_end(Some(strm));
            LzmaRet::ProgError
        }
    } else {
        let _ = internal;
        // lzma_end(Some(strm));
        LzmaRet::ProgError
    }
}

impl LzmaBlockDecoder {
    /// 获取块的实际大小信息
    pub fn get_block_info(&self) -> Option<LzmaBlock> {
        self.block.as_ref().map(|b| (**b).clone())
    }
}
