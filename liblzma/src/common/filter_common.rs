/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{
        LzmaFilter, LzmaOptionsBcj, LzmaOptionsDelta, LzmaOptionsLzma, LzmaOptionsType, LzmaRet,
        LzmaVli, LZMA_FILTERS_MAX, LZMA_FILTER_ARM, LZMA_FILTER_ARM64, LZMA_FILTER_ARMTHUMB,
        LZMA_FILTER_DELTA, LZMA_FILTER_IA64, LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA1EXT,
        LZMA_FILTER_LZMA2, LZMA_FILTER_POWERPC, LZMA_FILTER_SPARC, LZMA_FILTER_X86,
        LZMA_VLI_UNKNOWN,
    },
    common::LzmaFilterDecoder,
};

use super::{
    lzma_next_end, lzma_next_filter_init, LzmaFilterInfo, LzmaInitFunction, LzmaNextCoder,
    LZMA_MEMUSAGE_BASE,
};

pub type MemUsageFunction = fn(options: &LzmaOptionsType) -> u64;

/// 过滤器编码器和解码器的公共结构体
#[derive(Debug, Clone)]
pub struct LzmaFilterCoder {
    /// 过滤器 ID
    pub id: LzmaVli, // 假设 lzma_vli 是 u64 类型

    /// 初始化过滤器编码器并调用 lzma_next_filter_init() 用于 filters + 1
    pub init: Option<LzmaInitFunction>,

    /// 计算编码器的内存使用量。如果选项无效，则返回 u64::MAX
    pub memusage: Option<MemUsageFunction>,
}

// 实现 LzmaFilterCoder 的 Default 特性
impl Default for LzmaFilterCoder {
    fn default() -> Self {
        Self {
            // 默认过滤器 ID 设为 0
            id: 0,
            // 默认初始化函数，调用 lzma_next_filter_init 瞎给的函数，编译过了再说吧
            init: Some(lzma_next_filter_init),
            // 默认内存使用量计算函数为空
            memusage: None,
        }
    }
}

/// 查找过滤器的函数类型
pub type LzmaFilterFind = fn(id: LzmaVli) -> Option<LzmaFilterCoder>;

#[derive(Debug, Clone)]
struct FilterFeatures {
    /// 过滤器 ID
    id: LzmaVli, // 使用 LzmaVli 类型

    options: LzmaOptionsType,
    /// 过滤器特定选项结构体的大小
    options_size: usize,

    /// 如果此过滤器可以用作链中的非最后过滤器，则为 true
    non_last_ok: bool,

    /// 如果此过滤器可以用作链中的最后过滤器，则为 true
    last_ok: bool,

    /// 如果过滤器可能改变数据大小（即编码输出的数量可能与未压缩输入的数量不同），则为 true
    changes_size: bool,
}

/// 过滤器特性的静态数组
// static FEATURES: &[FilterFeatures] = &[
//     FilterFeatures {
//         id: LZMA_FILTER_LZMA1,
//         options: LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
//         options_size: std::mem::size_of::<LzmaOptionsLzma>(),
//         non_last_ok: false,
//         last_ok: true,
//         changes_size: true,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_LZMA1EXT,
//         options: LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
//         options_size: std::mem::size_of::<LzmaOptionsLzma>(),
//         non_last_ok: false,
//         last_ok: true,
//         changes_size: true,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_LZMA2,
//         options: LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
//         options_size: std::mem::size_of::<LzmaOptionsLzma>(),
//         non_last_ok: false,
//         last_ok: true,
//         changes_size: true,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_X86,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_POWERPC,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_IA64,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_ARM,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_ARMTHUMB,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_ARM64,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_SPARC,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsBcj>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_FILTER_DELTA,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: std::mem::size_of::<LzmaOptionsDelta>(),
//         non_last_ok: true,
//         last_ok: false,
//         changes_size: false,
//     },
//     FilterFeatures {
//         id: LZMA_VLI_UNKNOWN,
//         options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
//         options_size: 0,
//         non_last_ok: false,
//         last_ok: false,
//         changes_size: false,
//     },
// ];
use std::sync::LazyLock;

/// 过滤器特性的静态数组，使用 LazyLock 动态初始化
static FEATURES: LazyLock<[FilterFeatures; 12]> = LazyLock::new(|| {
    [
        FilterFeatures {
            id: LZMA_FILTER_LZMA1,
            options: LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
            options_size: std::mem::size_of::<LzmaOptionsLzma>(),
            non_last_ok: false,
            last_ok: true,
            changes_size: true,
        },
        FilterFeatures {
            id: LZMA_FILTER_LZMA1EXT,
            options: LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
            options_size: std::mem::size_of::<LzmaOptionsLzma>(),
            non_last_ok: false,
            last_ok: true,
            changes_size: true,
        },
        FilterFeatures {
            id: LZMA_FILTER_LZMA2,
            options: LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
            options_size: std::mem::size_of::<LzmaOptionsLzma>(),
            non_last_ok: false,
            last_ok: true,
            changes_size: true,
        },
        FilterFeatures {
            id: LZMA_FILTER_X86,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_POWERPC,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_IA64,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_ARM,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_ARMTHUMB,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_ARM64,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_SPARC,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: std::mem::size_of::<LzmaOptionsBcj>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_FILTER_DELTA,
            options: LzmaOptionsType::Delta(LzmaOptionsDelta::default()),
            options_size: std::mem::size_of::<LzmaOptionsDelta>(),
            non_last_ok: true,
            last_ok: false,
            changes_size: false,
        },
        FilterFeatures {
            id: LZMA_VLI_UNKNOWN,
            options: LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
            options_size: 0,
            non_last_ok: false,
            last_ok: false,
            changes_size: false,
        },
    ]
});

// 其他代码保持不变

pub fn lzma_filters_copy(src: &[LzmaFilter], real_dest: &mut [LzmaFilter]) -> LzmaRet {
    // println!("src {:#?}", src);
    if src.is_empty() || real_dest.is_empty() {
        return LzmaRet::ProgError;
    }

    // 使用临时目标，以确保在发生错误时不会修改实际目标
    let mut dest: [LzmaFilter; LZMA_FILTERS_MAX + 1] =
        core::array::from_fn(|_| LzmaFilter::default());

    let mut i = 0;
    let ret: LzmaRet = LzmaRet::Ok;

    while i < src.len() && src[i].id != LZMA_VLI_UNKNOWN {
        // println!("id {}", src[i].id);
        // 确保过滤器数量不超过最大值
        if i == LZMA_FILTERS_MAX {
            while i > 0 {
                // lzma_free(&mut dest[i].options, Some(allocator));
                i = i - 1;
            }
            return LzmaRet::OptionsError;
        }

        dest[i].id = src[i].id;

        if let Some(options) = &src[i].options {
            // 检查过滤器是否受支持
            let mut j = 0;
            while j < FEATURES.len() && src[i].id != FEATURES[j].id {
                if FEATURES[j].id == LZMA_VLI_UNKNOWN {
                    return LzmaRet::OptionsError;
                }
                j += 1;
            }

            // 分配并复制选项
            dest[i].options = Some(FEATURES[j].options.clone());
            if dest[i].options.is_none() {
                while i > 0 {
                    // lzma_free(&mut dest[i].options, Some(allocator));
                    i = i - 1;
                }
                return LzmaRet::MemError;
            }

            // 复制选项
            // 报错位置
            // unsafe{memcpy(dest[i].options.as_mut().unwrap() as *mut c_void, src[i].options as *mut c_void, FEATURES[j].options_size)};

            if let (Some(dest_option), Some(src_option)) =
                (dest[i].options.as_mut(), src[i].options.as_ref())
            {
                *dest_option = src_option.clone(); // 使用 Clone 特征进行安全拷贝
            } else {
                // 处理 None 的情况
                return LzmaRet::ProgError;
            }
        } else {
            dest[i].options = None;
        }

        i += 1;
    }

    // 终止过滤器数组
    assert!(i < LZMA_FILTERS_MAX + 1);

    dest[i].id = LZMA_VLI_UNKNOWN;
    dest[i].options = None;

    // 复制到调用者提供的数组
    // 报错位置
    // unsafe{memcpy(real_dest as *mut c_void, dest as *mut c_void, (i + 1) * std::mem::size_of::<LzmaFilter>())};
    for (dst, src) in real_dest.iter_mut().zip(dest.iter()) {
        *dst = src.clone();
    }
    LzmaRet::Ok
}

pub fn lzma_filters_free(filters: &mut [LzmaFilter]) {
    if filters.is_empty() {
        return;
    }
    // println!("filters = {:#?}", filters);
    let mut i = 0;
    while i < filters.len() && filters[i].id != LZMA_VLI_UNKNOWN {
        // println!("filters[i].id = {:?}", filters[i].id);
        // 检查是否超过最大过滤器数
        if i == LZMA_FILTERS_MAX {
            // API文档表示 LZMA_FILTERS_MAX + 1 是允许的最大大小，包括终止元素
            // 不应该到达这里，但如果出现 bug，我们不会越过数组的（可能的）结尾
            panic!("Unexpected filter count exceeds LZMA_FILTERS_MAX");
        }

        // 释放过滤器的选项
        filters[i].options = None;

        // 重置过滤器
        filters[i].id = LZMA_VLI_UNKNOWN;
        i += 1;
    }
}

pub fn lzma_validate_chain(filters: &[LzmaFilter], count: &mut usize) -> LzmaRet {
    // 必须至少有一个过滤器
    if filters.is_empty() || filters[0].id == LZMA_VLI_UNKNOWN {
        return LzmaRet::ProgError;
    }

    let mut changes_size_count = 0;
    let mut non_last_ok = true;
    let mut last_ok = false;

    let mut i = 0;
    while i < filters.len() && filters[i].id != LZMA_VLI_UNKNOWN {
        let mut j = 0;
        while j < FEATURES.len() && filters[i].id != FEATURES[j].id {
            if FEATURES[j].id == LZMA_VLI_UNKNOWN {
                return LzmaRet::OptionsError;
            }
            j += 1;
        }

        // 如果前一个过滤器不能作为非最后过滤器，则链无效
        if !non_last_ok {
            return LzmaRet::OptionsError;
        }

        non_last_ok = FEATURES[j].non_last_ok;
        last_ok = FEATURES[j].last_ok;
        changes_size_count += FEATURES[j].changes_size as usize;

        i += 1;
    }

    // 必须有 1-4 个过滤器。最后一个过滤器必须可用作链中的最后过滤器。
    // 最多允许三个过滤器改变数据大小。
    if i > LZMA_FILTERS_MAX || !last_ok || changes_size_count > 3 {
        return LzmaRet::OptionsError;
    }

    *count = i;
    LzmaRet::Ok
}

pub fn lzma_raw_coder_init(
    next: &mut LzmaNextCoder,
    options: &[LzmaFilter],
    coder_find: LzmaFilterFind,
    is_encoder: bool,
) -> LzmaRet {
    let mut count = 0;
    if lzma_validate_chain(options, &mut count) != LzmaRet::Ok {
        return LzmaRet::OptionsError;
    }

    let mut filters: [LzmaFilterInfo; LZMA_FILTERS_MAX + 1] =
        core::array::from_fn(|_| LzmaFilterInfo::default());

    if is_encoder {
        for i in 0..count {
            let j = count - i - 1;
            if let Some(fc) = coder_find(options[i].id) {
                // println!(
                //     "编码器: 原始ID = {:?}, 解码器ID = {:?}",
                //     options[i].id, fc.id
                // );
                if fc.init.is_none() {
                    return LzmaRet::OptionsError;
                }
                filters[j].id = options[i].id;
                filters[j].init = fc.init;
                filters[j].options = options[i].options.clone();
            } else {
                return LzmaRet::OptionsError;
            }
        }
    } else {
        for i in 0..count {
            if let Some(fc) = coder_find(options[i].id) {
                // println!(
                //     "解码器: 原始ID = {:?}, 解码器ID = {:?}, 解码器类型 = {:?}",
                //     options[i].id,
                //     fc.id,
                //     std::any::type_name_of_val(&fc)
                // );

                if fc.init.is_none() {
                    return LzmaRet::OptionsError;
                }

                // 确保使用原始ID
                filters[i].id = options[i].id;
                filters[i].init = fc.init;
                filters[i].options = options[i].options.clone();

                // println!(
                //     "设置后的过滤器: id = {:?}, init = {:?}",
                //     filters[i].id,
                //     std::any::type_name_of_val(&filters[i].init)
                // );
            } else {
                println!("未找到 ID 为 {:?} 的解码器", options[i].id);
                return LzmaRet::OptionsError;
            }
        }
    }

    filters[count].id = LZMA_VLI_UNKNOWN;
    filters[count].init = None;

    let ret = lzma_next_filter_init(next, &filters);
    if ret != LzmaRet::Ok {
        lzma_next_end(next);
    }
    ret
}

pub fn lzma_raw_coder_memusage(coder_find: LzmaFilterFind, filters: &[LzmaFilter]) -> u64 {
    let mut tmp = 0;
    if lzma_validate_chain(filters, &mut tmp) != LzmaRet::Ok {
        return u64::MAX;
    }

    let mut total = 0;
    let mut i = 0;

    while i < filters.len() && filters[i].id != LZMA_VLI_UNKNOWN {
        if let Some(fc) = coder_find(filters[i].id) {
            if let Some(memusage_fn) = fc.memusage {
                let usage = memusage_fn(filters[i].options.as_ref().unwrap());
                if usage == u64::MAX {
                    return u64::MAX;
                }
                total += usage;
            } else {
                total += 1024;
            }
        } else {
            return u64::MAX;
        }
        i += 1;
    }

    total + LZMA_MEMUSAGE_BASE
}
