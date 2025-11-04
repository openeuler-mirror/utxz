/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaBlock, LzmaVli},
    check::LzmaCheckState,
};

use super::LzmaNextCoder;

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
