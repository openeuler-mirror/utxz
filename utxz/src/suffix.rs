/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use lazy_static::lazy_static;
use std::ffi::CString;
use std::ptr;
use std::slice;
use std::str;
use std::sync::Mutex;

use crate::coder::FormatType;
use crate::coder::OperationMode;
use crate::coder::OPT_FORMAT;
use crate::coder::OPT_MODE;
use crate::util::xstrdup;

/// 全局变量：自定义后缀
// static mut CUSTOM_SUFFIX: Option<String> = None;
lazy_static! {
    static ref CUSTOM_SUFFIX: Mutex<String> = Mutex::new(String::new());
}

/// 检查字符是否为目录分隔符
///
/// \param c 要检查的字符
/// \return 如果是目录分隔符返回 true
fn is_dir_sep(c: char) -> bool {
    c == '/'
}

/// 检查字符串是否包含目录分隔符
///
/// \param str 要检查的字符串
/// \return 如果包含目录分隔符返回 true
fn has_dir_sep(s: &str) -> bool {
    s.contains('/')
}

/// 检查源文件名是否具有指定的压缩后缀
///
/// \param suffix      要查找的文件后缀
/// \param src_name    输入文件名
/// \param src_len     src_name的长度
///
/// \return 如果src_name有该后缀，返回src_len - suffix长度
///         这总是一个正整数。否则返回0
fn test_suffix(suffix: &str, src_name: &str, src_len: usize) -> usize {
    let suffix_len = suffix.len();

    // 文件名必须至少有一个字符加上后缀
    // src_name可能包含文件路径，所以我们也需要检查目录分隔符
    if src_len <= suffix_len
        || is_dir_sep(src_name[src_len - suffix_len - 1..].chars().next().unwrap())
    {
        return 0;
    }

    if src_name.ends_with(suffix) {
        return src_len - suffix_len;
    }

    0
}

/// 移除压缩文件的文件名后缀
///
/// \return 未压缩文件的名称，如果文件有未知后缀则返回None
fn uncompressed_name(src_name: &str, src_len: usize) -> Option<String> {
    let custom_suffix = CUSTOM_SUFFIX.lock().unwrap();

    struct Suffixes<'a> {
        compressed: &'a str,
        uncompressed: &'a str,
    }

    // 定义已知的后缀对应关系
    static SUFFIXES: [Suffixes; 4] = [
        Suffixes {
            compressed: ".xz",
            uncompressed: "",
        },
        Suffixes {
            compressed: ".txz",
            uncompressed: ".tar",
        }, // .txz作为.txt.gz的缩写很少见
        Suffixes {
            compressed: ".lzma",
            uncompressed: "",
        },
        Suffixes {
            compressed: ".tlz",
            uncompressed: ".tar",
        }, // 支持.tar.lzma和.tar.lz两种格式
    ];

    let mut new_suffix = "";
    let mut new_len = 0;

    // 检查已知后缀
    if *OPT_FORMAT.lock().unwrap() != FormatType::Raw {
        for i in SUFFIXES.iter() {
            new_len = test_suffix(i.compressed, src_name, src_len);
            if new_len != 0 {
                new_suffix = i.uncompressed;
                break;
            }
        }
    }

    // 检查自定义后缀
    if new_len == 0 && !custom_suffix.is_empty() {
        new_len = test_suffix(custom_suffix.as_str(), src_name, src_len);
    }

    if new_len == 0 {
        println!("{}: Filename has an unknown suffix, skipping", src_name);
        return None;
    }

    // 创建新的文件名：原始文件名（去掉后缀）+ 新后缀
    let mut dest_name = String::with_capacity(new_len + new_suffix.len());
    dest_name.push_str(&src_name[..new_len]);
    dest_name.push_str(new_suffix);

    Some(dest_name)
}

/// 显示后缀相关的警告消息
/// 在compressed_name()中多处需要此消息，所以将其放入单独的函数
fn msg_suffix(src_name: &str, suffix: &str) {
    println!("警告: {}: 文件已经有`{}`后缀，跳过", src_name, suffix);
}

/// 将后缀添加到src_name
///
/// 与uncompressed_name()相比，我们只检查指定文件格式的有效后缀
/// 获取压缩文件名
fn compressed_name(src_name: &str, src_len: usize) -> Option<String> {
    // 定义已知的后缀集合
    // let all_suffixes: &[&[&str]] = &[
    //     &[".xz", ".txz"],   // 对应 FORMAT_XZ 格式
    //     &[".lzma", ".tlz"], // 对应 FORMAT_LZMA 格式
    //     &[],                // 对应 --format=raw 的格式
    // ];
    let all_suffixes = [
        [".xz", ".txz", "", ""],
        [".lzma", ".tlz", "", ""],
        ["", "", "", ""],
    ];
    // 检查格式是否合法 (假设 `opt_format` 为 1 或 2)
    let opt_format = *OPT_FORMAT.lock().unwrap();
    assert!(opt_format != FormatType::Auto);

    let format = opt_format as usize - 1;
    // 获取当前格式的后缀数组
    let suffixes = all_suffixes[format];

    for i in suffixes.iter() {
        if !i.is_empty() {
            if test_suffix(i, src_name, src_len) != 0 {
                msg_suffix(src_name, i);
                return None;
            }
        }
    }

    let custom_suffix = CUSTOM_SUFFIX.lock().unwrap();
    // 查找已知文件名后缀并拒绝压缩它们
    if !custom_suffix.is_empty() {
        if test_suffix(&custom_suffix, src_name, src_len) != 0 {
            msg_suffix(src_name, &custom_suffix);
            return None;
        }
    }

    // 假设没有 custom_suffix，使用默认的第一个后缀
    let suffix = if custom_suffix.is_empty() {
        suffixes[0]
    } else {
        &custom_suffix as &str
    };

    // 构造新文件名
    let mut dest_name = String::with_capacity(src_len + suffix.len());
    dest_name.push_str(src_name);
    dest_name.push_str(suffix);

    Some(dest_name)
}

/// 获取目标文件名
///
/// \param src_name 源文件名
/// \return 根据操作模式返回压缩或解压缩后的文件名
pub fn suffix_get_dest_name(src_name: &str) -> Option<String> {
    assert!(!src_name.is_empty());

    let src_len = src_name.len();
    if *OPT_MODE.lock().unwrap() == OperationMode::Compress {
        compressed_name(src_name, src_len)
    } else {
        uncompressed_name(src_name, src_len)
    }
}

/// 设置自定义后缀
///
/// \param suffix 要设置的后缀
/// 空后缀和包含目录分隔符的后缀会被拒绝
fn suffix_set(suffix: &str) {
    let mut custom_suffix = CUSTOM_SUFFIX.lock().unwrap();
    if suffix.is_empty() || has_dir_sep(suffix) {
        panic!("无效的文件名后缀");
    }

    *custom_suffix = xstrdup(suffix);
}

/// 检查是否设置了自定义后缀
///
/// \return 如果设置了自定义后缀返回true
pub fn suffix_is_set() -> bool {
    let custom_suffix = CUSTOM_SUFFIX.lock().unwrap();
    Some(custom_suffix).is_some()
}
