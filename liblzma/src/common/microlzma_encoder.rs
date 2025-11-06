/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::{
    
    LzmaNextCoder,
};

/// MicroLZMA 编码器结构体
#[derive(Debug, Default)]
pub struct LzmaMicrolzmaEncoder {
    /// LZMA1 编码器
    lzma: Box<LzmaNextCoder>,

    /// LZMA 属性字节 (lc/lp/pb)
    props: u8,
}
