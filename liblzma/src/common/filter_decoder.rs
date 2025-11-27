/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::{
    api::{
        LzmaAction, LzmaFilter, LzmaOptionsType, LzmaRet, LzmaStream, LzmaVli, LZMA_FILTER_ARM,
        LZMA_FILTER_ARM64, LZMA_FILTER_ARMTHUMB, LZMA_FILTER_DELTA, LZMA_FILTER_IA64,
        LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA1EXT, LZMA_FILTER_LZMA2, LZMA_FILTER_POWERPC,
        LZMA_FILTER_SPARC, LZMA_FILTER_X86,
    },
    common::LzmaFilterCoder,
    delta::{lzma_delta_coder_memusage, lzma_delta_decoder_init, lzma_delta_props_decode},
    lzma::{
        lzma_lzma2_decoder_init, lzma_lzma2_decoder_memusage, lzma_lzma2_props_decode,
        lzma_lzma_decoder_init, lzma_lzma_decoder_memusage, lzma_lzma_props_decode,
    },
    simple::{
        lzma_simple_arm64_decoder_init, lzma_simple_arm_decoder_init,
        lzma_simple_armthumb_decoder_init, lzma_simple_ia64_decoder_init,
        lzma_simple_powerpc_decoder_init, lzma_simple_props_decode, lzma_simple_sparc_decoder_init,
        lzma_simple_x86_decoder_init,
    },
};

use super::{
    lzma_end, lzma_raw_coder_init, lzma_raw_coder_memusage, lzma_strm_init, LzmaFilterFind,
    LzmaInitFunction, LzmaNextCoder,
};

/// 过滤器解码器结构体
#[derive(Clone)]
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

impl Default for LzmaFilterDecoder {
    fn default() -> Self {
        Self {
            id: 0,
            init: None,
            memusage: None,
            props_decode: None,
        }
    }
}

/// 过滤器解码器数组
static DECODERS: &[LzmaFilterDecoder] = &[
    LzmaFilterDecoder {
        id: LZMA_FILTER_LZMA1,
        init: Some(lzma_lzma_decoder_init),
        memusage: Some(lzma_lzma_decoder_memusage),
        props_decode: Some(lzma_lzma_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_LZMA1EXT,
        init: Some(lzma_lzma_decoder_init),
        memusage: Some(lzma_lzma_decoder_memusage),
        props_decode: Some(lzma_lzma_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_LZMA2,
        init: Some(lzma_lzma2_decoder_init),
        memusage: Some(lzma_lzma2_decoder_memusage),
        props_decode: Some(lzma_lzma2_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_X86,
        init: Some(lzma_simple_x86_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_POWERPC,
        init: Some(lzma_simple_powerpc_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_IA64,
        init: Some(lzma_simple_ia64_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_ARM,
        init: Some(lzma_simple_arm_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_ARMTHUMB,
        init: Some(lzma_simple_armthumb_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_ARM64,
        init: Some(lzma_simple_arm64_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_SPARC,
        init: Some(lzma_simple_sparc_decoder_init),
        memusage: None,
        props_decode: Some(lzma_simple_props_decode),
    },
    LzmaFilterDecoder {
        id: LZMA_FILTER_DELTA,
        init: Some(lzma_delta_decoder_init),
        memusage: Some(lzma_delta_coder_memusage),
        props_decode: Some(lzma_delta_props_decode),
    },
];

fn decoder_find_base(id: LzmaVli) -> Option<LzmaFilterCoder> {
    // ENCODERS.iter().find(|encoder| encoder.id == id)
    for decoder in DECODERS.iter() {
        if decoder.id == id {
            let mut filterDecoder = LzmaFilterCoder::default();
            filterDecoder.id = decoder.id;
            filterDecoder.init = decoder.init;
            filterDecoder.memusage = decoder.memusage;
            return Some(filterDecoder.clone());
        }
    }
    None
}

/// 在解码器数组中查找指定 ID 的解码器
fn decoder_find(id: LzmaVli) -> Option<LzmaFilterDecoder> {
    // println!("找到匹配的解码器: ID = {:?}", id);
    // println!("准备查找的解码器 ID: {:?}", id);
    // println!("可用的解码器列表:");
    for decoder in DECODERS.iter() {
        // println!("正在比较: 当前解码器 ID {:?} vs 目标 ID {:?}", decoder.id, id);
        if decoder.id == id {
            // println!("找到匹配的解码器: ID = {:?}", id);
            // 返回解码器的副本而不是引用
            return Some(decoder.clone());
        }
    }
    None
}

/// 检查指定 ID 的解码器是否支持
#[no_mangle]
pub fn lzma_filter_decoder_is_supported(id: LzmaVli) -> bool {
    decoder_find(id).is_some()
}

/// 初始化原始解码器
pub fn lzma_raw_decoder_init(next: &mut LzmaNextCoder, options: &[LzmaFilter]) -> LzmaRet {
    lzma_raw_coder_init(next, options, decoder_find_base, false)
}

/// 创建原始解码器
pub fn lzma_raw_decoder(strm: &mut LzmaStream, options: &[LzmaFilter]) -> LzmaRet {
    // lzma_next_strm_init(lzma_raw_decoder_init, strm, options);
    let ret: LzmaRet = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 避免借用冲突的初始化
    let init_ret = match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                if let Some(ref mut next) = internal.next {
                    lzma_raw_decoder_init(next, options)
                } else {
                    LzmaRet::ProgError
                }
            } else {
                LzmaRet::ProgError
            }
        }
        Err(_) => LzmaRet::ProgError,
    };

    if init_ret != LzmaRet::Ok {
        lzma_end(Some(strm));
        return init_ret;
    }

    // 设置支持的操作
    match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                internal.supported_actions[LzmaAction::Run as usize] = true;
                internal.supported_actions[LzmaAction::Finish as usize] = true;
                LzmaRet::Ok
            } else {
                LzmaRet::ProgError
            }
        }
        Err(_) => LzmaRet::ProgError,
    }
}

/// 计算原始解码器所需的内存使用量
pub fn lzma_raw_decoder_memusage(filters: &[LzmaFilter]) -> u64 {
    lzma_raw_coder_memusage(
        unsafe {
            std::mem::transmute::<fn(LzmaVli) -> Option<LzmaFilterDecoder>, LzmaFilterFind>(
                decoder_find,
            )
        },
        filters,
    )
}

/// 解码属性
pub fn lzma_properties_decode(filter: &mut LzmaFilter, props: &[u8], props_size: usize) -> LzmaRet {
    // 确保 options 为 NULL
    filter.options = None;

    // 查找解码器
    let fd = match decoder_find(filter.id) {
        Some(fd) => fd,
        None => return LzmaRet::OptionsError,
    };

    // 如果没有属性解码函数，检查 props_size
    if fd.props_decode.is_none() {
        return if props_size == 0 {
            LzmaRet::Ok
        } else {
            LzmaRet::OptionsError
        };
    }

    // 调用属性解码函数
    if let Some(props_decode) = fd.props_decode {
        let (ret, options) = props_decode(props, props_size);
        filter.options = options;
        ret
    } else {
        LzmaRet::OptionsError
    }
}
