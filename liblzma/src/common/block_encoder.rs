/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaBlock, LzmaVli, LZMA_CHECK_SIZE_MAX, LZMA_VLI_MAX},
    check::LzmaCheckState,
};

use super::LzmaNextCoder;

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
