/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::collections::HashMap;
use std::{mem::offset_of, sync::LazyLock};

use crate::api::{
    LzmaDeltaType, LzmaFilter, LzmaOptionsType, LzmaVli, LZMA_FILTERS_MAX, LZMA_STR_ALL_FILTERS,
    LZMA_STR_DECODER, LZMA_STR_ENCODER, LZMA_STR_GETOPT_LONG, LZMA_STR_NO_SPACES,
    LZMA_STR_NO_VALIDATION, LZMA_VLI_UNKNOWN,
};
use crate::common::lzma_validate_chain;
use crate::LzmaOptionsDelta;
use crate::{
    api::{
        LzmaMatchFinder, LzmaMode, LzmaOptionsBcj, LzmaOptionsLzma, LzmaRet, LZMA_DELTA_DIST_MAX,
        LZMA_DELTA_DIST_MIN, LZMA_DICT_SIZE_MIN, LZMA_FILTER_ARM, LZMA_FILTER_ARM64,
        LZMA_FILTER_ARMTHUMB, LZMA_FILTER_DELTA, LZMA_FILTER_IA64, LZMA_FILTER_LZMA1,
        LZMA_FILTER_LZMA2, LZMA_FILTER_POWERPC, LZMA_FILTER_SPARC, LZMA_FILTER_X86, LZMA_LCLP_MAX,
        LZMA_LCLP_MIN, LZMA_PB_MAX, LZMA_PB_MIN, LZMA_PRESET_DEFAULT, LZMA_PRESET_EXTREME,
    },
    lzma::lzma_lzma_preset,
};
use common::my_min;

use super::LZMA_FILTER_RESERVED_START;

// use super::lzma_free;

const STR_ALLOC_SIZE: usize = 800;

pub struct LzmaStr {
    buf: String,
    pos: usize,
}

impl LzmaStr {
    // 添加 append_str 方法
    pub fn append_str(&mut self, s: &str) {
        let len = s.len();
        let limit = STR_ALLOC_SIZE - 1 - self.pos;
        let copy_size = my_min(len, limit);

        // 只复制允许长度的字符串
        self.buf.push_str(&s[..copy_size]);
        self.pos += copy_size;
    }

    /// 将一个 `u32` 值转换为字符串并追加到 `LzmaStr`
    pub fn append_u32(&mut self, value: u32, use_byte_suffix: bool) {
        // 将 `u32` 转换为字符串
        let mut value_str = value.to_string();

        // 如果需要字节后缀，则追加 "B"
        if use_byte_suffix {
            value_str.push('B');
        }

        // 将结果追加到 `LzmaStr`
        self.append_str(&value_str);
    }
}

impl LzmaStr {
    pub fn new() -> Self {
        LzmaStr {
            buf: String::with_capacity(STR_ALLOC_SIZE),
            pos: 0,
        }
    }

    // 可根据需要添加其他方法，例如缓冲区管理等
}

// 假设 lzma_alloc 函数已经在其他地方定义
fn lzma_alloc(size: usize) -> Option<String> {
    // 在 Rust 中，我们可以使用 String::with_capacity 来分配内存
    Some(String::with_capacity(size))
}

pub fn str_init(str: &mut LzmaStr) -> LzmaRet {
    match lzma_alloc(STR_ALLOC_SIZE) {
        Some(buf) => {
            str.buf = buf;
            str.pos = 0;
            LzmaRet::Ok
        }
        None => LzmaRet::MemError,
    }
}

/// 释放字符串资源
pub fn str_free(str: &mut LzmaStr) {
    // 在 Rust 中，内存管理通常由所有权系统自动处理
    // 如果需要手动释放，可以在这里实现
    // let mut tmp:Box<dyn std::any::Any> = Box::new(str.buf);
    // lzma_free(&mut Some(&mut tmp), Some(allocator));
}

/// 检查字符串是否已满
pub fn str_is_full(str: &LzmaStr) -> bool {
    str.pos == STR_ALLOC_SIZE - 1
}

/// 完成字符串处理并返回结果
pub fn str_finish(dest: &mut Option<String>, str: &mut LzmaStr) -> LzmaRet {
    if str_is_full(str) {
        // 预分配的缓冲区太小
        // 这种情况不应该发生，因为 STR_ALLOC_SIZE 应该
        // 在添加新过滤器时进行调整
        // let mut tmp:Box<dyn std::any::Any> = Box::new(str.buf);
        // lzma_free(&mut Some(&mut tmp), Some(allocator));
        *dest = None; // 设定 dest 为 None，类似于 C 中的 *dest = NULL
                      // 断言错误
        assert!(false);
        return LzmaRet::ProgError;
    }

    // 在字符串末尾添加空字符
    str.buf.push('\0');
    *dest = Some(str.buf.clone()); // 将字符串赋给 dest
    LzmaRet::Ok
}

/// 追加字符串
pub fn str_append_str(str: &mut LzmaStr, s: &str) {
    let len = s.len();
    let limit = STR_ALLOC_SIZE - 1 - str.pos;
    let copy_size = my_min(len, limit);

    // 只复制允许长度的字符串
    str.buf.push_str(&s[..copy_size]);
    str.pos += copy_size;
}

/// 追加 32 位无符号整数到字符串
/// 可能存在问题
pub fn str_append_u32(str: &mut LzmaStr, mut v: u32, use_byte_suffix: bool) {
    if v == 0 {
        str_append_str(str, "0");
    } else {
        // 定义后缀
        let suffixes = ["", "KiB", "MiB", "GiB"];

        let mut suf = 0;
        if use_byte_suffix {
            while (v & 1023) == 0 && suf < suffixes.len() - 1 {
                v >>= 10;
                suf += 1;
            }
        }

        // 构建数字字符串
        let mut buf = String::with_capacity(16); // 创建一个足够大的字符串
        let mut temp = v;
        // let mut pos = buf.len() -1;

        while temp != 0 {
            buf.push(char::from_digit((temp % 10) as u32, 10).unwrap());
            temp /= 10;
        }

        // 将数字字符串反转并追加到目标字符串
        for ch in buf.chars().rev() {
            str.buf.push(ch);
        }

        // 追加后缀
        str_append_str(str, suffixes[suf]);
    }
}

//////////////////////////////////////////////
// 解析和字符串化声明
//////////////////////////////////////////////

/// 过滤器和选项名称的最大长度
/// 11 字符 + 终止符 '\0' + size_of::<u32>() = 16 字节
pub const NAME_LEN_MAX: usize = 11;

/// option_map.flags 的标志位：使用 .u.map 将输入值转换为整数。
/// 如果没有这个标志，则使用 .u.range.{min,max} 作为整数的允许范围。
pub const OPTMAP_USE_NAME_VALUE_MAP: u8 = 0x01;

/// option_map.flags 的标志位：在输入字符串中允许 KiB/MiB/GiB，
/// 并在字符串化输出中使用它们（如果值是这些单位的精确倍数）。
/// 例如，用于 LZMA1/2 字典大小。
pub const OPTMAP_USE_BYTE_SUFFIX: u8 = 0x02;

/// option_map.flags 的标志位：如果整数值为零，
/// 则此选项不会包含在字符串化输出中。
/// 例如，用于 BCJ 过滤器起始偏移量（通常为零）。
pub const OPTMAP_NO_STRFY_ZERO: u8 = 0x04;

/// option_map.type 的可能值
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptMapType {
    Uint32, // 由于是第一个变体，它的值为 0
    LzmaMode,
    LzmaMatchFinder,
    LzmaPreset,
}

/// 名称-值映射表的结构体
#[derive(Debug)]
pub struct NameValueMap {
    pub name: &'static str, // 名称，使用静态字符串
    pub value: u32,         // 值
}
impl Clone for NameValueMap {
    fn clone(&self) -> Self {
        NameValueMap {
            name: self.name,
            value: self.value,
        }
    }
}
/// 选项映射表的结构体
#[derive(Debug)]
pub struct OptionMap {
    pub name: &'static str, // 名称，使用动态字符串
    pub type_: u8,          // 类型字段
    pub flags: u8,          // 标志字段
    pub offset: u16,        // 偏移量字段
    pub u: OptionMapUnion,  // 联合体，用于存储范围或映射表
}

/// 联合体，用于存储范围或映射表
#[derive(Debug)]
pub enum OptionMapUnion {
    /// 范围类型，包含最小值和最大值
    Range { min: u32, max: u32 },
    /// 名称-值映射表
    Map(&'static [NameValueMap]),
}

/// BCJ 选项映射表
pub static BCJ_OPTMAP: &[OptionMap] = &[OptionMap {
    name: "start",                                           // 选项名称
    flags: OPTMAP_NO_STRFY_ZERO | OPTMAP_USE_BYTE_SUFFIX,    // 标志
    offset: offset_of!(LzmaOptionsBcj, start_offset) as u16, // 偏移量
    u: OptionMapUnion::Range {
        min: 0,        // 最小值
        max: u32::MAX, // 最大值
    },
    type_: 0, // 假设默认值为 0
}];

/// 解析 BCJ 选项
///
/// # 参数
/// - `str_ptr`: 输入字符串指针，表示当前解析位置
/// - `str_end`: 输入字符串的结束位置
/// - `filter_options`: 用于存储解析结果的过滤器选项
///
/// # 返回值
/// 如果解析成功，返回 `None`；如果解析失败，返回错误信息
pub fn parse_bcj(
    str_ptr: &str, // 修改为 &mut &str
    str_end: &str,
    filter_options: LzmaOptionsType,
) -> Option<String> {
    parse_options(str_ptr, str_end, filter_options, BCJ_OPTMAP).err() // 将 Result<(), String> 转换为 Option<String>
}

/// Delta 选项映射表
pub static DELTA_OPTMAP: &[OptionMap] = &[OptionMap {
    name: "dist",                                      // 选项名称
    offset: offset_of!(LzmaOptionsDelta, dist) as u16, // 偏移量
    u: OptionMapUnion::Range {
        min: LZMA_DELTA_DIST_MIN, // 最小值
        max: LZMA_DELTA_DIST_MAX, // 最大值
    },
    flags: 0, // 假设默认值为 0
    type_: 0, // 假设默认值为 0
}];

/// 解析 Delta 过滤器选项
///
/// # 参数
/// - `str_ptr`: 输入字符串的引用，会在解析过程中更新
/// - `str_end`: 输入字符串的结束位置
/// - `filter_options`: 用于存储解析结果的过滤器选项
///
/// # 返回值
/// 如果解析成功，返回 `None`；如果解析失败，返回错误信息
pub fn parse_delta(
    str_ptr: &str,
    str_end: &str,
    filter_options: LzmaOptionsType,
) -> Option<String> {
    let mut opts: LzmaOptionsDelta = match filter_options {
        LzmaOptionsType::Delta(opts) => opts,
        _ => {
            return Some("Invalid filter options type".to_string());
        }
    };
    // 设置默认值
    opts.type_ = LzmaDeltaType::Byte; // LZMA_DELTA_TYPE_BYTE;
    opts.dist = LZMA_DELTA_DIST_MIN;

    // 调用通用选项解析函数
    parse_options(str_ptr, str_end, LzmaOptionsType::Delta(opts), DELTA_OPTMAP).err()
}

/// LZMA1 和 LZMA2 的预设字符串
const LZMA12_PRESET_STR: &str = "0-9[e]";

/// 解析 LZMA12 预设字符串，返回 Ok(()) 表示成功，Err(错误信息) 表示失败
pub fn parse_lzma12_preset(s: String, preset: &mut u32) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Some(("预设字符串为空").to_string());
    }
    // 取第一个字符作为预设数字
    *preset = (bytes[0] - b'0') as u32;

    // NOTE: 如果这里修改，记得同步更新 LZMA12_PRESET_STR！
    for &c in &bytes[1..] {
        match c as char {
            'e' => *preset |= LZMA_PRESET_EXTREME,
            _ => return Some(("不支持的预设标志").to_string()),
        }
    }
    None
}

/// 设置 LZMA12 预设参数，返回 Ok(()) 表示成功，Err(错误信息) 表示失败
pub fn set_lzma12_preset(s: String, filter_options: &mut LzmaOptionsLzma) -> Option<String> {
    // 解析预设字符串
    let mut preset = 0u32;
    let errmsg = parse_lzma12_preset(s, &mut preset);
    if errmsg.is_some() {
        return errmsg;
    }
    // 设置选项
    if !lzma_lzma_preset(filter_options, preset) {
        return Some(("不支持的预设参数").to_string());
    }

    None
}

// LZMA12 模式映射表
// 然后可以这样定义常量

const LZMA12_MODE_MAP: &[NameValueMap; 3] = &[
    NameValueMap {
        name: "fast",
        value: 1,
    },
    NameValueMap {
        name: "normal",
        value: 2,
    },
    NameValueMap { name: "", value: 0 },
];

const LZMA_MF_HC3: u32 = 0x03;
const LZMA_MF_HC4: u32 = 0x04;
const LZMA_MF_BT2: u32 = 0x12;
const LZMA_MF_BT3: u32 = 0x13;
const LZMA_MF_BT4: u32 = 0x14;
// LZMA12 匹配查找器映射表
const LZMA12_MF_MAP: &[NameValueMap; 6] = &[
    NameValueMap {
        name: "hc3",
        value: LZMA_MF_HC3,
    },
    NameValueMap {
        name: "hc4",
        value: LZMA_MF_HC4,
    },
    NameValueMap {
        name: "bt2",
        value: LZMA_MF_BT2,
    },
    NameValueMap {
        name: "bt3",
        value: LZMA_MF_BT3,
    },
    NameValueMap {
        name: "bt4",
        value: LZMA_MF_BT4,
    },
    NameValueMap { name: "", value: 0 },
];

// LZMA12 选项映射表
const LZMA12_OPTMAP: &[OptionMap] = &[
    OptionMap {
        name: "preset",
        type_: OptMapType::LzmaPreset as u8,
        flags: 0,
        offset: 0,
        u: OptionMapUnion::Range { min: 0, max: 0 },
    },
    OptionMap {
        name: "dict",
        flags: OPTMAP_USE_BYTE_SUFFIX,
        offset: offset_of!(LzmaOptionsLzma, dict_size) as u16,
        u: OptionMapUnion::Range {
            min: LZMA_DICT_SIZE_MIN,
            max: (1u32 << 30) + (1u32 << 29), // FIXME? 编码时最大值为这个，但解码允许4GiB-1B
        },

        type_: 0,
    },
    OptionMap {
        name: "lc",
        offset: offset_of!(LzmaOptionsLzma, lc) as u16,
        u: OptionMapUnion::Range {
            min: LZMA_LCLP_MIN,
            max: LZMA_LCLP_MAX,
        },
        type_: 0,
        flags: 0,
    },
    OptionMap {
        name: "lp",
        offset: offset_of!(LzmaOptionsLzma, lp) as u16,
        u: OptionMapUnion::Range {
            min: LZMA_LCLP_MIN,
            max: LZMA_LCLP_MAX,
        },

        type_: 0,
        flags: 0,
    },
    OptionMap {
        name: "pb",
        offset: offset_of!(LzmaOptionsLzma, pb) as u16,
        u: OptionMapUnion::Range {
            min: LZMA_PB_MIN,
            max: LZMA_PB_MAX,
        },

        type_: 0,
        flags: 0,
    },
    OptionMap {
        name: "mode",
        type_: OptMapType::LzmaMode as u8,
        flags: OPTMAP_USE_NAME_VALUE_MAP,
        offset: offset_of!(LzmaOptionsLzma, mode) as u16,
        u: OptionMapUnion::Map(LZMA12_MODE_MAP),
    },
    OptionMap {
        name: "nice",
        offset: offset_of!(LzmaOptionsLzma, nice_len) as u16,
        u: OptionMapUnion::Range { min: 2, max: 273 },

        type_: 0,
        flags: 0,
    },
    OptionMap {
        name: "mf",
        type_: OptMapType::LzmaMatchFinder as u8,
        flags: OPTMAP_USE_NAME_VALUE_MAP,
        offset: offset_of!(LzmaOptionsLzma, mf) as u16,
        u: OptionMapUnion::Map(LZMA12_MF_MAP),
    },
    OptionMap {
        name: "depth",
        offset: offset_of!(LzmaOptionsLzma, depth) as u16,
        u: OptionMapUnion::Range {
            min: 0,
            max: u32::MAX,
        },

        type_: 0,
        flags: 0,
    },
];

/// 解析 LZMA12 过滤器选项
///
/// # Arguments
/// * `str_ptr` - 输入字符串的引用的引用，会在解析过程中更新
/// * `str_end` - 输入字符串的结束位置
/// * `filter_options` - LZMA 选项结构体
///
/// # Returns
/// * `Option<String>` - 如果解析出错，返回错误信息；如果成功，返回 None
pub fn parse_lzma12(
    str_ptr: &str,
    str_end: &str,
    filter_options: LzmaOptionsType,
) -> Option<String> {
    let mut opts: LzmaOptionsLzma = match filter_options {
        LzmaOptionsType::LzmaOptionsLzma(opts) => opts,
        _ => {
            return Some("Invalid filter options type".to_string());
        }
    };
    // 设置默认预设值
    // 注意：在 Rust 中，我们假设 lzma_lzma_preset 总是成功
    let preset_ret = lzma_lzma_preset(&mut opts, LZMA_PRESET_DEFAULT);
    assert!(!preset_ret);
    // 解析选项
    parse_options(
        str_ptr,
        str_end,
        LzmaOptionsType::LzmaOptionsLzma(opts.clone()),
        LZMA12_OPTMAP,
    )
    .err();

    // 验证 lc + lp 不超过最大值
    if opts.lc + opts.lp > LZMA_LCLP_MAX {
        return Some("The sum of lc and lp must not exceed 4".to_string());
    }

    None
}

// enum LzmaOptionsType {
//     LZMA(LzmaOptionsLzma),
//     BCJ(LzmaOptionsBcj),
//     DELTA(LzmaOptionsDelta),
// }

/// 过滤器名称映射的结构体
#[derive(Debug)]
pub struct FilterNameMap {
    /// 过滤器名称
    pub name: &'static str,

    /// 选项大小
    pub opts_size: u32,

    /// 过滤器 ID
    pub id: u64, // 对应 C 中的 `lzma_vli`

    /// 解析函数指针
    pub parse: fn(&str, &str, LzmaOptionsType) -> Option<String>,

    /// 选项映射表
    pub optmap: &'static [OptionMap],

    /// 字符串化编码器的标志
    pub strfy_encoder: u8,

    /// 字符串化解码器的标志
    pub strfy_decoder: u8,

    /// 是否允许空值
    pub allow_null: bool,
}

/// 静态过滤器名称映射表
pub static FILTER_NAME_MAP: &[FilterNameMap] = &[
    FilterNameMap {
        name: "lzma1",
        opts_size: std::mem::size_of::<LzmaOptionsLzma>() as u32,
        id: LZMA_FILTER_LZMA1,
        parse: parse_lzma12,
        optmap: &LZMA12_OPTMAP,
        strfy_encoder: 9,
        strfy_decoder: 5,
        allow_null: false,
    },
    FilterNameMap {
        name: "lzma2",
        opts_size: std::mem::size_of::<LzmaOptionsLzma>() as u32,
        id: LZMA_FILTER_LZMA2,
        parse: parse_lzma12,
        optmap: &LZMA12_OPTMAP,
        strfy_encoder: 9,
        strfy_decoder: 2,
        allow_null: false,
    },
    FilterNameMap {
        name: "x86",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_X86,
        parse: parse_bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "arm",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_ARM,
        parse: parse_bcj,
        optmap: &BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "armthumb",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_ARMTHUMB,
        parse: parse_bcj,
        optmap: BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "arm64",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_ARM64,
        parse: parse_bcj,
        optmap: BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "powerpc",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_POWERPC,
        parse: parse_bcj,
        optmap: BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "ia64",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_IA64,
        parse: parse_bcj,
        optmap: BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "sparc",
        opts_size: std::mem::size_of::<LzmaOptionsBcj>() as u32,
        id: LZMA_FILTER_SPARC,
        parse: parse_bcj,
        optmap: BCJ_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: true,
    },
    FilterNameMap {
        name: "delta",
        opts_size: std::mem::size_of::<LzmaOptionsDelta>() as u32,
        id: LZMA_FILTER_DELTA,
        parse: parse_delta,
        optmap: DELTA_OPTMAP,
        strfy_encoder: 1,
        strfy_decoder: 1,
        allow_null: false,
    },
];

/// 解析过滤器选项
///
/// # 参数
/// - `str_ptr`: 输入字符串的引用，会在解析过程中更新
/// - `str_end`: 输入字符串的结束位置
/// - `filter_options`: 用于存储解析结果的过滤器选项
/// - `optmap`: 选项映射表
///
/// # 返回值
/// 如果解析成功，返回 `Ok(())`；如果解析失败，返回错误信息
pub fn parse_options(
    mut str_ptr: &str,
    str_end: &str,
    mut filter_options: LzmaOptionsType,
    optmap: &[OptionMap],
) -> Result<(), String> {
    while str_ptr < str_end && !str_ptr.is_empty() {
        // 跳过多余的逗号
        if str_ptr.starts_with(',') {
            str_ptr = &str_ptr[1..];
            continue;
        }

        // 找到下一个 name=value 的结束位置
        let name_eq_value_end = str_ptr.find(',').unwrap_or(str_end.len());
        let equals_sign = str_ptr.find('=');

        // 如果没有找到 '=' 或选项名称为空，则返回错误
        if equals_sign.is_none() || str_ptr.starts_with('=') {
            return Err("选项必须是 'name=value' 格式，并用逗号分隔".to_string());
        }

        let equals_sign = equals_sign.unwrap();
        let name_len = equals_sign;

        // 检查选项名称是否过长
        if name_len > NAME_LEN_MAX {
            return Err("未知的选项名称".to_string());
        }

        // 在 optmap 中查找选项名称
        let opt = optmap
            .iter()
            .find(|opt| opt.name.len() == name_len && opt.name == &str_ptr[..name_len]);

        if opt.is_none() {
            return Err("未知的选项名称".to_string());
        }

        let opt = opt.unwrap();
        str_ptr = &str_ptr[equals_sign + 1..];

        // 检查选项值是否为空
        let value_len = name_eq_value_end - equals_sign - 1;
        if value_len == 0 {
            return Err("选项值不能为空".to_string());
        }

        // 解析选项值
        let value = &str_ptr[..value_len];
        let parsed_value = if opt.flags & OPTMAP_USE_NAME_VALUE_MAP != 0 {
            // 从名称-值映射表中查找值
            let map = match opt.u {
                OptionMapUnion::Map(map) => map,
                _ => return Err("选项值映射表为空".to_string()),
            };

            let map_entry = map.iter().find(|entry| entry.name == value);

            if map_entry.is_none() {
                return Err("无效的选项值".to_string());
            }

            map_entry.unwrap().value
        } else if value.chars().all(|c| c.is_ascii_digit()) {
            // 解析为整数值
            value.parse::<u32>().map_err(|_| "值超出范围".to_string())?
        } else {
            return Err("值不是非负整数".to_string());
        };

        // 检查值是否在范围内
        if let OptionMapUnion::Range { min, max } = opt.u {
            if parsed_value < min || parsed_value > max {
                return Err("值超出范围".to_string());
            }
        }

        // 设置选项值到 filter_options
        let offset = opt.offset as usize;
        let ptr = &mut filter_options as *mut _ as *mut u8;
        let target = unsafe { ptr.add(offset) as *mut u32 };
        unsafe {
            *target = parsed_value;
        }

        // 更新 str_ptr 到下一个选项
        str_ptr = &str_ptr[name_eq_value_end..];
    }

    Ok(())
}

/// 解析过滤器
pub fn parse_filter(
    mut s: String,
    str_end: String,
    filter: &mut LzmaFilter,
    only_xz: bool,
) -> Option<String> {
    // 查找过滤器名称和选项的分隔符（冒号或等号）
    let mut name_end = str_end.clone();
    let mut opts_start = str_end.clone();

    // 假设 s 是待处理的字符串切片，代表 *str 到 str_end 的内容。
    if let Some(pos) = s.find(|c: char| c == ':' || c == '=') {
        // name_end 指向分隔符的位置
        let name_end = pos;
        // opts_start 指向分隔符后第一个字符的位置
        let opts_start = pos + 1;
        // 之后可根据 name_end 和 opts_start 进行后续处理
    } else {
        // 没有找到分隔符则不做处理
    }

    // 检查过滤器名称长度是否过长
    let name_len = name_end.len();
    if name_len > NAME_LEN_MAX {
        return Some(("Unknown filter name").to_string());
    }

    // 在过滤器名称映射表中查找匹配的过滤器
    for entry in FILTER_NAME_MAP {
        if entry.name == name_end {
            if only_xz && entry.id >= LZMA_FILTER_RESERVED_START {
                return Some(("This filter cannot be used in the .xz format").to_string());
            }

            // 初始化过滤器选项
            let mut options = match entry.id {
                0x21 => LzmaOptionsType::LzmaOptionsLzma(LzmaOptionsLzma::default()),
                0x03 => LzmaOptionsType::Delta(LzmaOptionsDelta::default()),
                0x04 => LzmaOptionsType::Bcj(LzmaOptionsBcj::default()),
                _ => return Some(("Unsupported filter").to_string()),
            };

            // 调用过滤器特定的解析函数
            s = opts_start; // 直接赋值给 String
            if let Some(errmsg) =
                (entry.parse)(s.as_str(), str_end.clone().as_str(), options.clone())
            {
                return Some(errmsg);
            }

            // 如果解析成功，设置过滤器的 ID 和选项
            filter.id = entry.id;
            filter.options = Some(options);
            return None;
        }
    }

    Some(("Unknown filter name").to_string())
}

/// 检查字符是否为数字
fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// 跳过字符串中的空格
fn skip_spaces(s: String) -> String {
    s.trim_start().to_string()
}
/// 将字符串转换为过滤器链
///
/// # 参数
/// - `input`: 输入字符串，表示过滤器链
/// - `filter_map`: 全局过滤器名称映射表
/// - `flags`: 标志，用于控制解析行为
///
/// # 返回值
/// 如果解析成功，返回过滤器链；如果解析失败，返回错误信息
/// 将字符串转换为过滤器
pub fn str_to_filters(mut s: String, filters: &mut [LzmaFilter], flags: u32) -> Option<String> {
    let mut errmsg: Option<String> = None;

    // 跳过前导空格
    s.trim_start().to_string();

    if s.clone().is_empty() {
        return Some(
            ("Empty string is not allowed, try \"6\" if a default value is needed").to_string(),
        );
    }

    // 检测字符串类型
    if is_digit(s.clone().chars().next().unwrap())
        || (s.starts_with('-') && s.len() > 1 && is_digit(s.chars().nth(1).unwrap()))
    {
        if s.starts_with('-') {
            s = s[1..].to_string();
        }

        // 忽略尾随空格
        let str_end = s.find(' ').unwrap_or_else(|| s.len());
        let preset_str = &s[..str_end];
        s = s[str_end..].to_string();

        let mut preset = 0;
        let filter_str_owned = str_end.to_string();
        errmsg = parse_lzma12_preset(s, &mut preset);
        if errmsg.is_some() {
            return errmsg;
        }

        let mut opts = LzmaOptionsLzma::default();
        if lzma_lzma_preset(&mut opts, preset) {
            return Some(("Unsupported preset").to_string());
        }

        filters[0] = LzmaFilter {
            id: LZMA_FILTER_LZMA2,
            options: Some(LzmaOptionsType::LzmaOptionsLzma(opts)),
        };
        filters[1] = LzmaFilter::default();

        return None;
    }

    // 解析过滤器链
    let only_xz = (flags & LZMA_STR_ALL_FILTERS) == 0;
    let mut temp_filters = vec![LzmaFilter::default(); LZMA_FILTERS_MAX + 1];

    let mut i = 0;
    while !s.clone().is_empty() {
        if i == LZMA_FILTERS_MAX {
            errmsg = Some(("The maximum number of filters is four").to_string());
            break;
        }

        if s.clone().starts_with("--") {
            s = s[2..].to_string();
        }

        let filter_end = s
            .find("--")
            .or_else(|| s.find(' '))
            .unwrap_or_else(|| s.len());
        let filter_str = s[..filter_end].to_string(); // 将切片转换为 String
        s = s[filter_end..].to_string();

        errmsg = parse_filter(
            s.clone(),
            filter_str.to_string(),
            &mut temp_filters[i],
            only_xz,
        );
        if errmsg.is_some() {
            break;
        }

        skip_spaces(s.clone());
        i += 1;
    }

    if errmsg.is_none() {
        temp_filters[i] = LzmaFilter::default();

        if (flags & LZMA_STR_NO_VALIDATION) == 0 && !validate_filter_chain(&temp_filters[..i]) {
            errmsg = Some(("Invalid filter chain ('lzma2' missing at the end?)").to_string());
        } else {
            filters[..=i].clone_from_slice(&temp_filters[..=i]);
        }
    }

    if errmsg.is_some() {
        for filter in temp_filters.iter_mut().take(i) {
            filter.options = None; // 清理已分配的选项
        }
    }

    errmsg
}
/// 验证过滤器链是否有效
fn validate_filter_chain(filters: &[LzmaFilter]) -> bool {
    // 简单验证：检查是否以 LZMA2 结束
    if let Some(last_filter) = filters.last() {
        return last_filter.id == LZMA_FILTER_LZMA2;
    }
    false
}

// #[derive(Debug)]
// pub struct LzmaFilter {
//     pub id: u64,
//     pub options: Option<HashMap<String, u32>>,
// }

// #[derive(Debug)]
// pub struct FilterInfo {
//     pub id: u64,
//     pub opts_size: usize,
// }

/// 将字符串转换为过滤器链
///
/// # 参数
/// - `input`: 输入字符串，表示过滤器链
/// - `error_pos`: 错误位置的可变引用，用于记录解析失败的位置
/// - `filters`: 用于存储解析结果的过滤器数组
/// - `flags`: 标志，用于控制解析行为
/// - `filter_map`: 全局过滤器名称映射表
///
/// # 返回值
/// 如果解析成功，返回 `Ok(())`；如果解析失败，返回错误信息
// pub fn lzma_str_to_filters(
//     input: &str,
//     error_pos: &mut Option<usize>,
//     filters: &mut Vec<LzmaFilter>,
//     flags: u32,
//     filter_map: &HashMap<&str, FilterInfo>,
// ) -> Result<(), String> {
//     // 检查输入字符串和过滤器是否为空
//     if input.is_empty() || filters.is_empty() {
//         return Err("输入字符串或过滤器为空".to_string());
//     }

//     // 验证标志是否合法
//     const SUPPORTED_FLAGS: u32 = LZMA_STR_ALL_FILTERS | LZMA_STR_NO_VALIDATION;
//     if flags & !SUPPORTED_FLAGS != 0 {
//         return Err("不支持的标志".to_string());
//     }

//     let mut used = input;
//     let result = str_to_filters(used, filters, flags, filter_map);

//     if let Err(err) = result {
//         if let Some(pos) = error_pos {
//             *pos = input.len() - used.len();
//         }
//         return Err(err);
//     }

//     Ok(())
// }
pub const OPTMAP_TYPE_UINT32: u32 = 0;
pub const OPTMAP_TYPE_LZMA_MODE: u32 = 1;
pub const OPTMAP_TYPE_LZMA_MATCH_FINDER: u32 = 2;
pub const OPTMAP_TYPE_LZMA_PRESET: u32 = 3;

/// 选项访问trait，用于从不同类型的选项结构体中根据偏移量读取值
/// 这个trait抽象了不同选项类型的字段访问，避免了使用unsafe指针操作
trait OptionAccess {
    /// 根据字节偏移量和类型读取选项值
    ///
    /// # 参数
    /// - `offset`: 字段在结构体中的字节偏移量
    /// - `type_`: 选项类型，用于特殊处理某些字段（如枚举类型）
    ///
    /// # 返回值
    /// - `Some(u32)`: 成功读取到的值，转换为u32
    /// - `None`: 偏移量无效或字段不存在
    fn read_value_at_offset(&self, offset: u32, type_: u32) -> Option<u32>;
}

/// 为 LZMA 选项实现选项访问trait
impl OptionAccess for LzmaOptionsLzma {
    fn read_value_at_offset(&self, offset: u32, type_: u32) -> Option<u32> {
        // 根据偏移量直接访问结构体字段，避免了C风格的指针算术
        match offset {
            0 => Some(self.dict_size), // 字典大小（4字节）
            4 => Some(self.lc as u32), // literal context bits（1字节）
            5 => Some(self.lp as u32), // literal position bits（1字节）
            6 => Some(self.pb as u32), // position bits（1字节）
            8 => match type_ {
                // LZMA模式枚举需要特殊处理
                OPTMAP_TYPE_LZMA_MODE => Some(self.mode.clone() as u32),
                _ => Some(self.mode.clone() as u32),
            },
            12 => match type_ {
                // 匹配查找器枚举需要特殊处理
                OPTMAP_TYPE_LZMA_MATCH_FINDER => Some(self.mf.clone() as u32),
                _ => Some(self.mf.clone() as u32),
            },
            // 其他偏移量暂未实现，返回None
            // TODO: 添加对其他字段的支持（如nice_len, depth等）
            _ => None,
        }
    }
}

/// 为 Delta 过滤器选项实现选项访问trait
impl OptionAccess for LzmaOptionsDelta {
    fn read_value_at_offset(&self, offset: u32, type_: u32) -> Option<u32> {
        match offset {
            0 => Some(self.type_.clone() as u32), // Delta类型枚举（4字节）
            4 => Some(self.dist),                 // Delta距离（4字节）
            _ => None,                            // 无效偏移量
        }
    }
}

/// 为 BCJ（分支/调用/跳转）过滤器选项实现选项访问trait
impl OptionAccess for LzmaOptionsBcj {
    fn read_value_at_offset(&self, offset: u32, type_: u32) -> Option<u32> {
        match offset {
            0 => Some(self.start_offset), // 起始偏移量（4字节）
            _ => None,                    // BCJ选项只有一个字段
        }
    }
}

// 然后简化主函数
pub fn strfy_filter(
    dest: &mut LzmaStr,
    delimiter: &str,
    optmap: &[OptionMap],
    optmap_count: usize,
    filter_options: &LzmaOptionsType,
) {
    let mut current_delimiter = delimiter;

    for i in 0..optmap_count {
        let om = &optmap[i];

        if om.type_ == OPTMAP_TYPE_LZMA_PRESET as u8 {
            continue;
        }

        let v = match filter_options {
            LzmaOptionsType::LzmaOptionsLzma(opts) => {
                opts.read_value_at_offset(om.offset as u32, om.type_ as u32)
            }
            LzmaOptionsType::Delta(opts) => {
                opts.read_value_at_offset(om.offset as u32, om.type_ as u32)
            }
            LzmaOptionsType::Bcj(opts) => {
                opts.read_value_at_offset(om.offset as u32, om.type_ as u32)
            }
            LzmaOptionsType::Lod(opts) => {
                opts.read_value_at_offset(om.offset as u32, om.type_ as u32)
            }
            LzmaOptionsType::None => continue,
        };

        let v = match v {
            Some(value) => value,
            None => continue,
        };

        if v == 0 && (om.flags & OPTMAP_NO_STRFY_ZERO != 0) {
            continue;
        }

        dest.append_str(current_delimiter);
        current_delimiter = ",";

        dest.append_str(om.name);
        dest.append_str("=");

        if (om.flags & OPTMAP_USE_NAME_VALUE_MAP) != 0 {
            if let OptionMapUnion::Map(map) = &om.u {
                let mut found = false;
                for entry in map.iter() {
                    if entry.name.is_empty() {
                        dest.append_str("UNKNOWN");
                        found = true;
                        break;
                    }
                    if entry.value == v {
                        dest.append_str(entry.name);
                        found = true;
                        break;
                    }
                }
                if !found {
                    dest.append_str("UNKNOWN");
                }
            } else {
                dest.append_str("UNKNOWN");
            }
        } else {
            let use_byte_suffix = (om.flags & OPTMAP_USE_BYTE_SUFFIX) != 0;
            dest.append_u32(v, use_byte_suffix);
        }
    }
}

/// 将过滤器链转换为字符串
///
/// # 参数
/// - `output_str`: 用于存储生成的字符串
/// - `filters`: 过滤器数组
/// - `flags`: 标志，用于控制字符串生成行为
///
/// # 返回值
/// 如果成功，返回 `Ok(())`；如果失败，返回错误信息
/// 将过滤器链转换为字符串描述，
/// 参数 output_str 为输出字符串（返回 Some(String)）；
/// filters 为过滤器数组；flags 为控制标志；allocator 用于内存管理（此处无实际用途，借 Rust 内建内存管理替代）。
///
/// 返回 LzmaRet 类型，成功时返回 LZMA_OK，否则返回错误代码（例如 LzmaRet::ProgError 或 LZMA_OPTIONS_ERROR）。
pub fn lzma_str_from_filters(
    output_str: &mut Option<String>,
    filters: &[LzmaFilter],
    flags: u32,
) -> LzmaRet {
    // 1. 参数检查：如果 output_str 为 None（不可能，因为引用必定有效），或 filters 为空则返回编程错误
    *output_str = None;
    if filters.is_empty() {
        return LzmaRet::ProgError;
    }

    // 2. 验证 flags 是否为支持的标志
    const SUPPORTED_FLAGS: u32 =
        LZMA_STR_ENCODER | LZMA_STR_DECODER | LZMA_STR_GETOPT_LONG | LZMA_STR_NO_SPACES;
    if flags & !SUPPORTED_FLAGS != 0 {
        return LzmaRet::OptionsError;
    }

    // 3. 至少应当存在一个过滤器；如果第一个过滤器的 id 为 LZMA_VLI_UNKNOWN，则表示过滤器链为空，返回错误。
    if filters[0].id == LZMA_VLI_UNKNOWN {
        return LzmaRet::OptionsError;
    }

    // 4. 分配输出字符串（使用 Rust 的 String 动态管理，无需显式使用 allocator）
    let mut dest = LzmaStr::new();
    // 如果调用 str_init 有错误则返回；此处直接新建 String 表示成功。

    // 5. 根据 flags 判断是否需要显示过滤器选项，多个标志互相"或"
    let show_opts = (flags & (LZMA_STR_ENCODER | LZMA_STR_DECODER)) != 0;
    // 根据是否启用 getopt_long 语法设置过滤器选项的分隔符
    let opt_delim = if flags & LZMA_STR_GETOPT_LONG != 0 {
        "="
    } else {
        ":"
    };

    // 6. 遍历 filters 数组，直到遇到 ID 为 LZMA_VLI_UNKNOWN（过滤器链结束标记）
    for (i, filter) in filters.iter().enumerate() {
        if filter.id == LZMA_VLI_UNKNOWN {
            break;
        }

        // 如果达到过滤器最大数量，则释放掉已经添加的字符串并返回错误
        if i == LZMA_FILTERS_MAX {
            // 这里不需要释放 dest（Rust 自动管理内存），直接返回错误
            return LzmaRet::OptionsError;
        }

        // 7. 如果不是第一个过滤器且调用者允许在过滤器间添加空格（即未设置 LZMA_STR_NO_SPACES），则添加空格分隔
        if i > 0 && (flags & LZMA_STR_NO_SPACES == 0) {
            dest.append_str(" ");
        }

        // 8. 如果使用 getopt_long 语法或者（有多个过滤器且禁止空格），则在前面添加 "--"
        if (flags & LZMA_STR_GETOPT_LONG != 0) || (i > 0 && (flags & LZMA_STR_NO_SPACES != 0)) {
            dest.append_str("--");
        }

        // 9. 内部循环：查找 filter_name_map 中与当前 filter.id 匹配的条目
        let mut entry_found = None;
        for entry in FILTER_NAME_MAP.iter() {
            if entry.id == filter.id {
                entry_found = Some(entry);
                break;
            }
        }
        let entry = match entry_found {
            Some(e) => e,
            None => return LzmaRet::OptionsError, // 如果找不到匹配项，则返回错误
        };

        // 10. 将过滤器名称添加到输出字符串中
        dest.append_str(entry.name);

        // 当只要求显示过滤器名称时，跳过后续选项处理
        if !show_opts {
            continue;
        }

        // 11. 检查当前过滤器的 options 部分
        if filter.options.is_none() {
            // 如果过滤器不允许空选项，则返回错误
            if !entry.allow_null {
                return LzmaRet::OptionsError;
            }
            // 否则 options 允许为 None，则不添加附加选项，直接处理下一个过滤器
            continue;
        }

        // 12. 选项结构存在，根据 flags 判断使用编码还是解码时的字符串化方法
        let optmap_count = if flags & LZMA_STR_ENCODER != 0 {
            entry.strfy_encoder
        } else {
            entry.strfy_decoder
        };

        // 13. 调用 strfy_filter 将过滤器选项转换为字符串，并追加到 dest 中
        // 注意：此函数负责根据 opt_delim、optmap、optmap_count 以及 filter.options（此处保证为 Some）生成字符串描述
        strfy_filter(
            &mut dest,
            opt_delim,
            entry.optmap,
            optmap_count.into(),
            filter.options.as_ref().unwrap(),
        );
    }

    // 14. 如果未设置 LZMA_STR_NO_VALIDATION，则进行基本验证，
    // 调用 lzma_validate_chain 检查过滤器链是否有效（例如是否缺少必须的 lzma2 过滤器）。
    if flags & LZMA_STR_NO_VALIDATION == 0 {
        let mut dummy: usize = 0;
        let ret = lzma_validate_chain(filters, &mut dummy);
        // 此处断言返回值应为 LzmaRet::Ok 或 LzmaRet::OptionsError
        assert!(ret == LzmaRet::Ok || ret == LzmaRet::OptionsError);
        if ret != LzmaRet::Ok {
            return LzmaRet::OptionsError;
        }
    }

    // 15. 将最终字符串写入 output_str，并返回 LzmaRet::Ok 表示成功。
    *output_str = Some(dest.buf);
    LzmaRet::Ok
}

// pub fn lzma_str_list_filters(
//     output_str: &mut Option<String>,
//     filter_id: u64,
//     flags: u32,
// ) -> Result<(), String> {
//     // 如果 `output_str` 是空的，直接返回错误
//     if output_str.is_none() {
//         return Err("程序错误：output_str 为空".to_string());
//     }

//     *output_str = None;

//     // 验证标志是否合法
//     const SUPPORTED_FLAGS: u32 = LZMA_STR_ALL_FILTERS
//         | LZMA_STR_ENCODER
//         | LZMA_STR_DECODER
//         | LZMA_STR_GETOPT_LONG;
//     if flags & !SUPPORTED_FLAGS != 0 {
//         return Err("选项错误：不支持的标志".to_string());
//     }

//     // 初始化输出字符串
//     let mut dest = String::new();

//     // 是否显示选项
//     let show_opts = (flags & (LZMA_STR_ENCODER | LZMA_STR_DECODER)) != 0;

//     // 过滤器分隔符
//     let filter_delim = if show_opts { "\n" } else { " " };

//     // 选项分隔符
//     let opt_delim = if (flags & LZMA_STR_GETOPT_LONG) != 0 {
//         "="
//     } else {
//         ":"
//     };

//     let mut first_filter_printed = false;

//     for filter_info in FILTER_NAME_MAP.iter() {
//         // 如果指定了过滤器 ID，则跳过不匹配的过滤器
//         if filter_id != LZMA_VLI_UNKNOWN && filter_id != filter_info.id {
//             continue;
//         }

//         // 如果只列出 .xz 格式的过滤器，则跳过其他过滤器
//         if filter_info.id >= LZMA_FILTER_RESERVED_START
//             && (flags & LZMA_STR_ALL_FILTERS) == 0
//             && filter_id == LZMA_VLI_UNKNOWN
//         {
//             continue;
//         }

//         // 如果不是第一个过滤器，添加分隔符
//         if first_filter_printed {
//             dest.push_str(filter_delim);
//         }
//         first_filter_printed = true;

//         // 如果使用长选项格式，添加 "--"
//         if (flags & LZMA_STR_GETOPT_LONG) != 0 {
//             dest.push_str("--");
//         }

//         // 添加过滤器名称
//         dest.push_str(filter_info.name);

//         // 如果不需要显示选项，跳过选项处理
//         if !show_opts {
//             continue;
//         }

//         // 获取选项映射表
//         let optmap = filter_info.optmap;
//         let mut d = opt_delim;

//         let end = if (flags & LZMA_STR_ENCODER) != 0 {
//             filter_info.strfy_encoder.len()
//         } else {
//             filter_info.strfy_decoder.len()
//         };

//         for j in 0..end {
//             let opt = &optmap[j];

//             // 添加选项分隔符
//             dest.push_str(d);
//             d = ",";

//             // 添加选项名称
//             dest.push_str(opt.name);
//             dest.push_str("=<");

//             if opt.type_ == OPTMAP_TYPE_LZMA_PRESET {
//                 // LZMA1/2 预设有自定义的帮助字符串
//                 dest.push_str(LZMA12_PRESET_STR);
//             } else if opt.flags & OPTMAP_USE_NAME_VALUE_MAP != 0 {
//                 // 使用名称-值映射表
//                 if let OptionMapUnion::Map(map) = &opt.u {
//                     for (k, entry) in map.iter().enumerate() {
//                         if k > 0 {
//                             dest.push('|');
//                         }
//                         dest.push_str(entry.name);
//                     }
//                 }
//             } else {
//                 // 显示整数范围
//                 if opt.flags & OPTMAP_USE_BYTE_SUFFIX != 0 {
//                     dest.push_str(&format!("{}B-{}B", opt.u.range.min, opt.u.range.max));
//                 } else {
//                     dest.push_str(&format!("{}-{}", opt.u.range.min, opt.u.range.max));
//                 }
//             }

//             dest.push('>');
//         }
//     }

//     // 如果没有任何过滤器被添加到字符串中，返回错误
//     if !first_filter_printed {
//         return Err("选项错误：未找到匹配的过滤器".to_string());
//     }

//     // 将生成的字符串存储到 `output_str`
//     *output_str = Some(dest);

//     Ok(())
// }
