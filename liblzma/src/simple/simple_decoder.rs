/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

///////////////////////////////////////////////////////////////////////////////
//
/// \file       simple_decoder.rs
/// \brief      简单过滤器的属性解码器
//
//  Author:     Lasse Collin
//
//  This file has been put into the public domain.
//  You can do whatever you want with this file.
//
///////////////////////////////////////////////////////////////////////////////
use byteorder::{ByteOrder, LittleEndian};

use crate::api::{LzmaOptionsBcj, LzmaOptionsType, LzmaRet};

/// 解码简单过滤器的属性
///
/// # 参数
/// * `options` - 输出选项的可变引用
/// * `allocator` - LZMA分配器
/// * `props` - 属性数据
/// * `props_size` - 属性数据大小
///
/// # 返回值
/// * `LzmaRet` - 操作结果
pub fn lzma_simple_props_decode(
    props: &[u8],
    props_size: usize,
) -> (LzmaRet, Option<LzmaOptionsType>) {
    // 如果没有属性数据，直接返回成功
    if props_size == 0 {
        return (LzmaRet::Ok, None);
    }

    // 检查属性大小是否正确
    if props_size != 4 {
        return (LzmaRet::OptionsError, None);
    }

    // 创建新的BCJ选项结构体
    let mut opt = LzmaOptionsBcj::default();

    // 从小端字节序读取起始偏移量
    opt.start_offset = LittleEndian::read_u32(props);

    // 如果起始偏移量为0，不保存选项结构体
    let mut options: Option<LzmaOptionsType> = None;
    if opt.start_offset == 0 {
        options = None;
    } else {
        options = Some(LzmaOptionsType::Bcj(opt.clone()));
    }

    (LzmaRet::Ok, options)
}
