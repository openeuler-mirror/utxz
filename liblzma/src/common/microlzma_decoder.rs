/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::LzmaVli;

use super::LzmaNextCoder;

/// MicroLZMA 解码器状态
#[derive(Debug, Default)]
pub struct LzmaMicrolzmaDecoder {
    /// LZMA1 解码器
    lzma: Box<LzmaNextCoder>,

    /// 应用程序提供的流的压缩大小。
    /// 这个值必须完全正确。
    ///
    /// 当读取输入时，这个值会递减。
    comp_size: u64,

    /// 应用程序提供的流的解压大小。
    /// 如果 uncomp_size_is_exact 为 false，这个值可能小于实际解压大小。
    ///
    /// 当产生输出时，这个值会递减。
    uncomp_size: LzmaVli,

    /// 应用程序提供的 LZMA 字典大小
    dict_size: u32,

    /// 如果为 true，则表示确切的解压大小已知。
    /// 如果为 false，uncomp_size 可能小于实际解压大小；
    /// uncomp_size 永远不能大于实际解压大小。
    uncomp_size_is_exact: bool,

    /// 一旦处理了 MicroLZMA 流的第一个字节，则为 true。
    props_decoded: bool,
}
