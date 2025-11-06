/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::{LzmaOptionsType, LzmaRet};

use super::LzmaInitFunction;

/// 过滤器解码器结构体
#[derive(Clone, Default)]
pub struct LzmaFilterDecoder {
    /// 过滤器 ID
    pub id: u64, // lzma_vli 在 Rust 中通常对应 u64 类型

    /// 初始化过滤器编码器，并调用 lzma_next_filter_init()，参数为过滤器数量 + 1。
    pub init: Option<LzmaInitFunction>,

    /// 计算编码器的内存使用量。如果选项无效，返回 u64::MAX。
    pub memusage: Option<MemUsageFunction>,

    /// 解码过滤器属性。
    ///
    /// # 返回值
    /// - `LzmaRet::Ok`: 属性解码成功。
    /// - `LzmaRet::OptionsError`: 不支持的属性。
    /// - `LzmaRet::MemError`: 内存分配失败。
    pub props_decode: Option<PropsDecodeFunction>,
}

// 函数指针类型别名
pub type MemUsageFunction = fn(options: &LzmaOptionsType) -> u64;
pub type PropsDecodeFunction =
    fn(props: &[u8], props_size: usize) -> (LzmaRet, Option<LzmaOptionsType>);

impl LzmaFilterDecoder {
    /// 创建一个新的 LzmaFilterDecoder 实例，使用默认值
    pub fn new(id: u64) -> Self {
        Self {
            id,
            init: None,
            memusage: None,
            props_decode: None,
        }
    }
}

