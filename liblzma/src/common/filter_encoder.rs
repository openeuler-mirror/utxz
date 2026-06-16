/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![deny(clippy::absurd_extreme_comparisons)]
#![deny(clippy::useless_attribute)]
use crate::{
    api::{
        LzmaFilter, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream, LzmaVli,
        LZMA_FILTERS_MAX, LZMA_FILTER_ARM, LZMA_FILTER_ARM64, LZMA_FILTER_ARMTHUMB,
        LZMA_FILTER_DELTA, LZMA_FILTER_IA64, LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA1EXT,
        LZMA_FILTER_LZMA2, LZMA_FILTER_POWERPC, LZMA_FILTER_SPARC, LZMA_FILTER_X86,
    },
    common::LzmaFilterCoder,
    delta::{lzma_delta_coder_memusage, lzma_delta_encoder_init, lzma_delta_props_encode},
    lzma::{
        lzma_lzma2_encoder_init, lzma_lzma2_encoder_memusage, lzma_lzma2_props_encode,
        lzma_lzma_encoder_init, lzma_lzma_encoder_memusage, lzma_lzma_props_encode,
    },
    simple::{
        lzma_simple_arm64_encoder_init, lzma_simple_arm_encoder_init,
        lzma_simple_armthumb_encoder_init, lzma_simple_ia64_encoder_init,
        lzma_simple_powerpc_encoder_init, lzma_simple_props_encode, lzma_simple_props_size,
        lzma_simple_sparc_encoder_init, lzma_simple_x86_encoder_init,
    },
};

use super::{
    lzma_end, lzma_raw_coder_init, lzma_raw_coder_memusage, lzma_strm_init, LzmaFilterFind,
    LzmaInitFunction, LzmaNextCoder,
};
use crate::common::filter_common::MemUsageFunction;
/// 过滤器编码器结构体
#[derive(Debug, Clone)]
pub struct LzmaFilterEncoder {
    /// 过滤器 ID
    pub id: u64, // lzma_vli 在 Rust 中通常对应 u64 类型

    /// 初始化过滤器编码器，并调用 lzma_next_filter_init()，参数为过滤器数量 + 1。
    pub init: Option<LzmaInitFunction>,

    /// 计算编码器的内存使用量。如果选项无效，返回 u64::MAX。
    pub memusage: Option<MemUsageFunction>,

    /// 计算推荐的未压缩大小，以便将输入数据拆分为多个 .xz 块，使多线程编码成为可能。
    /// 如果为 None，则假定编码器在单线程下足够快。
    pub block_size: Option<BlockSizeFunction>,

    /// 告知过滤器属性字段的大小。如果选项无效，返回 LzmaRet::OptionsError，并将 size 设置为 u32::MAX。
    pub props_size_get: Option<PropsSizeGetFunction>,

    /// 某些过滤器的属性字段大小始终相同。如果 props_size_get 为 None，则使用此值。
    pub props_size_fixed: u32,

    /// 编码过滤器属性。
    ///
    /// # 返回值
    /// - `LzmaRet::Ok`: 属性编码成功。
    /// - `LzmaRet::OptionsError`: 不支持的选项。
    /// - `LzmaRet::ProgError`: 无效选项或输出空间不足。
    pub props_encode: Option<PropsEncodeFunction>,
}

pub type BlockSizeFunction = fn(options: &LzmaOptionsType) -> u64;
pub type PropsSizeGetFunction = fn(size: &mut u32, options: &LzmaOptionsType) -> LzmaRet;
pub type PropsEncodeFunction = fn(options: &LzmaOptionsType, out: &mut [u8]) -> LzmaRet;

impl LzmaFilterEncoder {
    /// 创建一个新的 LzmaFilterEncoder 实例，使用默认值
    pub fn new(id: u64) -> Self {
        Self {
            id,
            init: None,
            memusage: None,
            block_size: None,
            props_size_get: None,
            props_size_fixed: 0,
            props_encode: None,
        }
    }
}

impl Default for LzmaFilterEncoder {
    fn default() -> Self {
        Self {
            id: 0,
            init: None,
            memusage: None,
            block_size: None,
            props_size_get: None,
            props_size_fixed: 0,
            props_encode: None,
        }
    }
}

/// Wrapper to compute LZMA2 block size for multi-threaded encoding.
/// Block size is max(dict_size * 3, 1 MiB) to keep compression ratio reasonable.
fn lzma2_block_size(options: &LzmaOptionsType) -> u64 {
    match options {
        LzmaOptionsType::LzmaOptionsLzma(opt) => std::cmp::max((opt.dict_size as u64) * 3, 1 << 20),
        _ => u64::MAX,
    }
}

static ENCODERS: &[LzmaFilterEncoder] = &[
    LzmaFilterEncoder {
        id: LZMA_FILTER_LZMA1,
        init: Some(lzma_lzma_encoder_init),
        memusage: Some(lzma_lzma_encoder_memusage),
        block_size: None,
        props_size_get: None,
        props_size_fixed: 5,
        props_encode: Some(lzma_lzma_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_LZMA1EXT,
        init: Some(lzma_lzma_encoder_init),
        memusage: Some(lzma_lzma_encoder_memusage),
        block_size: None,
        props_size_get: None,
        props_size_fixed: 5,
        props_encode: Some(lzma_lzma_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_LZMA2,
        init: Some(lzma_lzma2_encoder_init),
        memusage: Some(lzma_lzma2_encoder_memusage),
        block_size: Some(lzma2_block_size),
        props_size_get: None,
        props_size_fixed: 1,
        props_encode: Some(lzma_lzma2_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_X86,
        init: Some(lzma_simple_x86_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_POWERPC,
        init: Some(lzma_simple_powerpc_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_IA64,
        init: Some(lzma_simple_ia64_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_ARM,
        init: Some(lzma_simple_arm_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_ARMTHUMB,
        init: Some(lzma_simple_armthumb_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_ARM64,
        init: Some(lzma_simple_arm64_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_SPARC,
        init: Some(lzma_simple_sparc_encoder_init),
        memusage: None,
        block_size: None,
        props_size_fixed: 0,
        props_size_get: Some(lzma_simple_props_size),
        props_encode: Some(lzma_simple_props_encode),
    },
    LzmaFilterEncoder {
        id: LZMA_FILTER_DELTA,
        init: Some(lzma_delta_encoder_init),
        memusage: Some(lzma_delta_coder_memusage),
        block_size: None,
        props_size_get: None,
        props_size_fixed: 1,
        props_encode: Some(lzma_delta_props_encode),
    },
];

/// 在编码器数组中查找指定 ID 的编码器
// fn encoder_find(id: LzmaVli) -> Option<&'static LzmaFilterEncoder> {
fn encoder_find_fn(id: LzmaVli) -> Option<LzmaFilterCoder> {
    // ENCODERS.iter().find(|encoder| encoder.id == id)
    for encoder in ENCODERS.iter() {
        if encoder.id == id {
            let mut filterCoder = LzmaFilterCoder::default();
            filterCoder.id = encoder.id;
            filterCoder.init = encoder.init;
            filterCoder.memusage = encoder.memusage;
            return Some(filterCoder.clone());
        }
    }
    None
}

fn encoder_find(id: LzmaVli) -> Option<LzmaFilterEncoder> {
    // ENCODERS.iter().find(|encoder| encoder.id == id)
    for encoder in ENCODERS.iter() {
        if encoder.id == id {
            return Some(encoder.clone());
        }
    }
    None
}

pub fn lzma_filter_encoder_is_supported(id: LzmaVli) -> bool {
    encoder_find(id).is_some()
}

pub fn lzma_filters_update(strm: &mut LzmaStream, filters: &[LzmaFilter]) -> LzmaRet {
    // let mut internal = strm.internal;

    if strm
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .next
        .as_mut()
        .unwrap()
        .update
        .is_none()
    {
        return LzmaRet::ProgError;
    }

    // Validate the filter chain
    if lzma_raw_encoder_memusage(filters) == u64::MAX {
        return LzmaRet::OptionsError;
    }

    // Create reversed filters
    let mut count = 1;
    while filters.get(count).map_or(false, |f| f.id != u64::MAX) {
        count += 1;
    }

    // let mut reversed_filters:Vec<LzmaFilter> = Vec::with_capacity(LZMA_FILTERS_MAX + 1);
    let mut reversed_filters: [LzmaFilter; LZMA_FILTERS_MAX + 1] =
        core::array::from_fn(|_| LzmaFilter::default());
    for i in 0..count {
        reversed_filters[count - i - 1] = filters[i].clone();
    }
    reversed_filters[count].id = u64::MAX;

    // Call the update function
    if let Some(update) = strm
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .next
        .as_mut()
        .unwrap()
        .update
    {
        return update(
            strm.internal
                .borrow_mut()
                .as_mut()
                .unwrap()
                .next
                .as_mut()
                .unwrap()
                .coder
                .as_mut()
                .unwrap(),
            Some(filters),
            &reversed_filters,
        );
    } else {
        LzmaRet::ProgError
    }
}

pub fn lzma_raw_encoder_init(next: &mut LzmaNextCoder, options: &[LzmaFilter]) -> LzmaRet {
    unsafe {
        lzma_raw_coder_init(
            next,
            options,
            // std::mem::transmute::<
            //     fn(id: LzmaVli) -> Option<&'static LzmaFilterEncoder>,
            //     LzmaFilterFind,
            // >(encoder_find),
            encoder_find_fn,
            true,
        )
    }
}

pub fn lzma_raw_encoder(strm: &mut LzmaStream, options: &[LzmaFilter]) -> LzmaRet {
    // lzma_next_strm_init(lzma_raw_coder_init, strm, options, LzmaFilterEncoder::find, true);
    let ret: LzmaRet = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 避免借用冲突的初始化
    let init_ret = unsafe {
        match strm.internal.try_borrow_mut() {
            Ok(mut internal_ref) => {
                if let Some(ref mut internal) = internal_ref.as_mut() {
                    if let Some(ref mut next) = internal.next {
                        lzma_raw_coder_init(
                            next,
                            options,
                            // std::mem::transmute::<
                            //     fn(id: LzmaVli) -> Option<&'static LzmaFilterEncoder>,
                            //     LzmaFilterFind,
                            // >(encoder_find),
                            encoder_find_fn,
                            true,
                        )
                    } else {
                        LzmaRet::ProgError
                    }
                } else {
                    LzmaRet::ProgError
                }
            }
            Err(_) => LzmaRet::ProgError,
        }
    };

    if init_ret != LzmaRet::Ok {
        lzma_end(Some(strm));
        return init_ret;
    }

    // 设置支持的操作
    match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                internal.supported_actions[0] = true; // RUN
                internal.supported_actions[1] = true; // SYNC_FLUSH
                internal.supported_actions[2] = true; // FINISH
            }
        }
        Err(_) => return LzmaRet::ProgError,
    }

    LzmaRet::Ok
}

pub fn lzma_raw_encoder_memusage(filters: &[LzmaFilter]) -> u64 {
    unsafe {
        lzma_raw_coder_memusage(
            // std::mem::transmute::<
            //     fn(id: LzmaVli) -> Option<&'static LzmaFilterEncoder>,
            //     LzmaFilterFind,
            // >(encoder_find),
            encoder_find_fn,
            filters,
        )
    }
}

pub fn lzma_mt_block_size(filters: &[LzmaFilter]) -> u64 {
    let mut max = 0;

    for filter in filters.iter().take_while(|f| f.id != u64::MAX) {
        if let Some(fe) = encoder_find(filter.id) {
            if let Some(block_size_fn) = fe.block_size {
                let size = block_size_fn(filter.options.as_ref().unwrap());
                if size == 0 {
                    return 0;
                }
                max = max.max(size);
            }
        }
    }

    max
}

pub fn lzma_properties_size(size: &mut u32, filter: &LzmaFilter) -> LzmaRet {
    let fe = match encoder_find(filter.id) {
        Some(fe) => fe,
        None => {
            return if filter.id <= LzmaVli::MAX / 2 {
                LzmaRet::OptionsError
            } else {
                LzmaRet::ProgError
            };
        }
    };

    if let Some(props_size_get) = fe.props_size_get {
        props_size_get(size, filter.options.as_ref().unwrap())
    } else {
        *size = fe.props_size_fixed;
        LzmaRet::Ok
    }
}

pub fn lzma_properties_encode(filter: &LzmaFilter, props: &mut [u8]) -> LzmaRet {
    let fe = match encoder_find(filter.id) {
        Some(fe) => fe,
        None => return LzmaRet::ProgError,
    };

    if let Some(props_encode) = fe.props_encode {
        props_encode(filter.options.as_ref().unwrap(), props)
    } else {
        LzmaRet::Ok
    }
}
