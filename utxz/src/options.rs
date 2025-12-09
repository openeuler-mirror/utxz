/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::util::{str_to_uint64, xstrdup};
use liblzma::api::{
    LzmaOptionsBcj, LzmaOptionsDelta, LzmaOptionsLzma, LZMA_DELTA_DIST_MAX, LZMA_DELTA_DIST_MIN,
    LZMA_DICT_SIZE_MIN, LZMA_LCLP_MAX, LZMA_LCLP_MIN, LZMA_MF_BT2, LZMA_MF_BT3, LZMA_MF_BT4,
    LZMA_MF_HC3, LZMA_MF_HC4, LZMA_MODE_FAST, LZMA_MODE_NORMAL, LZMA_PB_MAX, LZMA_PB_MIN,
    LZMA_PRESET_DEFAULT, LZMA_PRESET_EXTREME,
};

/// `NameIdMap` 结构体，包含一个名称和一个 ID
#[derive(Debug, Clone)]
pub struct NameIdMap {
    pub name: Option<&'static str>, // None 表示终止项
    pub id: u64,
}

/// `OptionMap` 结构体，包含一个名称，一个 `NameIdMap` 映射以及最小和最大值
pub struct OptionMap {
    pub name: Option<&'static str>,
    pub map: Option<&'static [NameIdMap]>,
    pub min: u64,
    pub max: u64,
}

/// 解析选项字符串并调用 `set` 函数处理每个选项
pub fn parse_options(
    str: Option<&str>,
    opts: &'static [OptionMap],
    set: &mut dyn FnMut(&mut dyn FilterOptions, usize, u64, Option<&str>),
    filter_options: &mut dyn FilterOptions,
) -> Result<(), String> {
    let str = match str {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(()), // 如果字符串为空或为None，直接返回
    };

    for option in str.split(',') {
        let mut parts = option.splitn(2, '=');
        let name = parts.next().ok_or(format!("Invalid option: {}", option))?;
        let value = parts
            .next()
            .ok_or(format!("Missing value for option: {}", name))?;

        // 查找选项名称在映射表中的位置
        let option_map = opts
            .iter()
            .find(|opt| opt.name.map_or(false, |n| n == name))
            .ok_or(format!("Invalid option name: {}", name))?;

        // 处理选项值
        if let Some(map) = option_map.map {
            // 值是一个字符串，需要映射为整数
            let mapped_value = map
                .iter()
                .find(|m| m.name.map_or(false, |n| n == value))
                .ok_or(format!("Invalid option value: {}", value))?;

            set(
                filter_options,
                option_map as *const _ as usize,
                mapped_value.id,
                Some(value),
            );
        } else if option_map.min == u64::MAX {
            // 值是一个特殊字符串，由 `set` 函数解析
            set(
                filter_options,
                option_map as *const _ as usize,
                0,
                Some(value),
            );
        } else {
            // 值是一个整数
            let v = str_to_uint64(name, value, option_map.min, option_map.max);
            set(
                filter_options,
                option_map as *const _ as usize,
                v,
                Some(value),
            );
        }
    }

    Ok(())
}

/// 定义 `FilterOptions` trait，用于抽象 `set` 函数的行为
pub trait FilterOptions {
    fn set(&mut self, key: usize, value: u64, valuestr: Option<&str>);
}

// 补充缺失的常量定义
const UINT64_MAX: u64 = u64::MAX;

// 替代 C 风格的宏定义，使用 Rust 常量
const OPT_DIST: usize = 0;

// 定义 DeltaOptions 结构体，用于封装 delta 压缩选项
pub struct DeltaOptions {
    dist: u64,
}

impl DeltaOptions {
    /// 设置 delta 压缩选项
    pub fn set_delta(
        &mut self,
        key: usize,
        value: u64,
        _valuestr: Option<&str>,
    ) -> Result<(), String> {
        match key {
            OPT_DIST => {
                if value < LZMA_DELTA_DIST_MIN.into() || value > LZMA_DELTA_DIST_MAX.into() {
                    return Err(format!("Invalid delta distance: {}", value));
                }
                self.dist = value;
                Ok(())
            }
            _ => Err(format!("Unknown option key: {}", key)),
        }
    }
}

// 创建并配置 DeltaOptions 实例
pub fn options_delta(str: Option<&str>) -> Result<DeltaOptions, String> {
    static OPTS: [OptionMap; 2] = [
        OptionMap {
            name: Some("dist"),
            map: None,
            min: LZMA_DELTA_DIST_MIN as u64,
            max: LZMA_DELTA_DIST_MAX as u64,
        },
        OptionMap {
            name: None,
            map: None,
            min: 0,
            max: 0,
        },
    ];

    let mut options = DeltaOptions {
        dist: LZMA_DELTA_DIST_MIN.into(),
    };

    parse_options(
        str,
        &OPTS,
        &mut |filter_options, key, value, valuestr| filter_options.set(key, value, valuestr),
        &mut options,
    )?;

    Ok(options)
}

impl FilterOptions for DeltaOptions {
    fn set(&mut self, key: usize, value: u64, valuestr: Option<&str>) {
        self.set_delta(key, value, valuestr)
            .expect("Invalid delta option");
    }
}

// 新增 Rust 风格的 BCJ 选项设置函数
pub struct BcjOptions {
    start_offset: u64,
}

impl BcjOptions {
    pub fn set_bcj(
        &mut self,
        key: usize,
        value: u64,
        _valuestr: Option<&str>,
    ) -> Result<(), String> {
        match key {
            OPT_START_OFFSET => {
                if value > u32::MAX as u64 {
                    return Err(format!("Invalid start offset: {}", value));
                }
                self.start_offset = value;
                Ok(())
            }
            _ => Err(format!("Unknown option key: {}", key)),
        }
    }
}

const OPT_START_OFFSET: usize = 0;

pub fn options_bcj(str: Option<&str>) -> Result<BcjOptions, String> {
    static OPTS: [OptionMap; 2] = [
        OptionMap {
            name: Some("start"),
            map: None,
            min: 0,
            max: u32::MAX as u64,
        },
        OptionMap {
            name: None,
            map: None,
            min: 0,
            max: 0,
        },
    ];

    let mut options = BcjOptions { start_offset: 0 };

    parse_options(
        str,
        &OPTS,
        &mut |filter_options, key, value, valuestr| filter_options.set(key, value, valuestr),
        &mut options,
    )?;

    Ok(options)
}
impl FilterOptions for BcjOptions {
    fn set(&mut self, key: usize, value: u64, _valuestr: Option<&str>) {
        // 调用具体的 set_bcj 方法进行设置
        self.set_bcj(key, value, _valuestr)
            .expect("Invalid BCJ option");
    }
}

/// 定义 LZMA 选项结构体
pub struct LzmaOptions {
    preset: u32,
    dict_size: u64,
    lc: u32,
    lp: u32,
    pb: u32,
    mode: u32,
    nice_len: u32,
    mf: u32,
    depth: u32,
}

impl LzmaOptions {
    /// 设置 LZMA 选项
    pub fn set_lzma(
        &mut self,
        key: usize,
        value: u64,
        valuestr: Option<&str>,
    ) -> Result<(), String> {
        match key {
            OPT_PRESET => {
                if let Some(s) = valuestr {
                    if s.chars().nth(0).map_or(false, |c| c < '0' || c > '9') {
                        return Err(format!("Unsupported LZMA1/LZMA2 preset: {}", s));
                    }

                    let mut preset = s.chars().nth(0).unwrap() as u32 - '0' as u32;

                    if s.len() > 1 {
                        if s.chars().nth(1).unwrap() == 'e' {
                            preset |= LZMA_PRESET_EXTREME;
                        } else {
                            return Err(format!("Unsupported LZMA1/LZMA2 preset: {}", s));
                        }

                        if s.len() > 2 {
                            return Err(format!("Unsupported LZMA1/LZMA2 preset: {}", s));
                        }
                    }

                    self.preset = preset;
                }
                Ok(())
            }
            OPT_DICT => {
                self.dict_size = value;
                Ok(())
            }
            OPT_LC => {
                self.lc = value as u32;
                Ok(())
            }
            OPT_LP => {
                self.lp = value as u32;
                Ok(())
            }
            OPT_PB => {
                self.pb = value as u32;
                Ok(())
            }
            OPT_MODE => {
                self.mode = value as u32;
                Ok(())
            }
            OPT_NICE => {
                self.nice_len = value as u32;
                Ok(())
            }
            OPT_MF => {
                self.mf = value as u32;
                Ok(())
            }
            OPT_DEPTH => {
                self.depth = value as u32;
                Ok(())
            }
            _ => Err(format!("Unknown option key: {}", key)),
        }
    }
}

// 替代 C 风格的宏定义，使用 Rust 常量
const OPT_PRESET: usize = 0;
const OPT_DICT: usize = 1;
const OPT_LC: usize = 2;
const OPT_LP: usize = 3;
const OPT_PB: usize = 4;
const OPT_MODE: usize = 5;
const OPT_NICE: usize = 6;
const OPT_MF: usize = 7;
const OPT_DEPTH: usize = 8;

/// 创建并配置 LzmaOptions 实例
pub fn options_lzma(str: Option<&str>) -> Result<LzmaOptions, String> {
    static MODES: &[NameIdMap] = &[
        NameIdMap {
            name: Some("fast"),
            id: LZMA_MODE_FAST,
        },
        NameIdMap {
            name: Some("normal"),
            id: LZMA_MODE_NORMAL,
        },
    ];

    static MFS: &[NameIdMap] = &[
        NameIdMap {
            name: Some("hc3"),
            id: LZMA_MF_HC3,
        },
        NameIdMap {
            name: Some("hc4"),
            id: LZMA_MF_HC4,
        },
        NameIdMap {
            name: Some("bt2"),
            id: LZMA_MF_BT2,
        },
        NameIdMap {
            name: Some("bt3"),
            id: LZMA_MF_BT3,
        },
        NameIdMap {
            name: Some("bt4"),
            id: LZMA_MF_BT4,
        },
    ];

    static OPTS: [OptionMap; 9] = [
        OptionMap {
            name: Some("preset"),
            map: None,
            min: u64::MAX,
            max: 0,
        },
        OptionMap {
            name: Some("dict"),
            map: None,
            min: LZMA_DICT_SIZE_MIN as u64,
            max: (1 << 30) + (1 << 29),
        },
        OptionMap {
            name: Some("lc"),
            map: None,
            min: LZMA_LCLP_MIN as u64,
            max: LZMA_LCLP_MAX as u64,
        },
        OptionMap {
            name: Some("lp"),
            map: None,
            min: LZMA_LCLP_MIN as u64,
            max: LZMA_LCLP_MAX as u64,
        },
        OptionMap {
            name: Some("pb"),
            map: None,
            min: LZMA_PB_MIN as u64,
            max: LZMA_PB_MAX as u64,
        },
        OptionMap {
            name: Some("mode"),
            map: Some(&MODES),
            min: 0,
            max: 0,
        },
        OptionMap {
            name: Some("nice"),
            map: None,
            min: 2,
            max: 273,
        },
        OptionMap {
            name: Some("mf"),
            map: Some(&MFS),
            min: 0,
            max: 0,
        },
        OptionMap {
            name: Some("depth"),
            map: None,
            min: 0,
            max: u32::MAX as u64,
        },
    ];

    let mut options = LzmaOptions {
        preset: LZMA_PRESET_DEFAULT,
        dict_size: 0,
        lc: 0,
        lp: 0,
        pb: 0,
        mode: 0,
        nice_len: 0,
        mf: 0,
        depth: 0,
    };

    parse_options(
        str,
        &OPTS,
        &mut |filter_options, key, value, valuestr| filter_options.set(key, value, valuestr),
        &mut options,
    )?;

    if options.lc + options.lp > LZMA_LCLP_MAX as u32 {
        return Err("The sum of lc and lp must not exceed 4".to_string());
    }

    Ok(options)
}

impl FilterOptions for LzmaOptions {
    fn set(&mut self, key: usize, value: u64, valuestr: Option<&str>) {
        self.set_lzma(key, value, valuestr)
            .expect("Invalid LZMA option");
    }
}
