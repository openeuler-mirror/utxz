/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(unused_variables)]
#![warn(unused_assignments)]
use lazy_static::lazy_static;
use std::sync::{Arc, Mutex, Weak};

/// 当前的操作模式
#[derive(Debug, PartialEq, Clone)]
#[repr(u32)]
pub enum OperationMode {
    Compress,
    Decompress,
    Test,
    List,
}

/// 当前的格式类型
#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u32)]
pub enum FormatType {
    Auto,
    Xz,
    Lzma,
    Lzip,
    Raw,
    // 你可以根据实际需要添加更多格式类型
}

lazy_static! {

    /// 当前操作模式，默认为 MODE_COMPRESS
    pub static ref OPT_MODE: Mutex<OperationMode> = Mutex::new(OperationMode::Compress);

 /// 当前文件格式，默认为 FORMAT_AUTO
    pub static ref OPT_FORMAT: Mutex<FormatType> = Mutex::new(FormatType::Auto);

    /// 自动调整标志，默认为 true
    pub static ref OPT_AUTO_ADJUST: Mutex<bool> = Mutex::new(true);

    /// 单流模式标志，默认为 false
    pub static ref OPT_SINGLE_STREAM: Mutex<bool> = Mutex::new(false);

    /// 块大小，用于分块压缩，默认为 0
    pub static ref OPT_BLOCK_SIZE: Mutex<u64> = Mutex::new(0);

    /// 块列表，存放各块大小，初始为 None
    pub static ref OPT_BLOCK_LIST: Mutex<Option<Vec<u64>>> = Mutex::new(None);

    /// 过滤器数量，零表示使用预设，默认为 0
    pub static ref FILTERS_COUNT: Mutex<u32> = Mutex::new(0);

    /// 默认完整性检查标志，当使用 --check=CHECK 选项后设为 false，默认为 true
    pub static ref CHECK_DEFAULT: Mutex<bool> = Mutex::new(true);

    /// 允许存在未消费输入的标志，解码成功后生效，默认为 false
    pub static ref ALLOW_TRAILING_INPUT: Mutex<bool> = Mutex::new(false);
}
