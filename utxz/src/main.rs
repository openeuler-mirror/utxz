/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

mod args;
mod coder;
mod file_io;
mod hardware;
mod list;
mod message;
mod mytime;
mod options;
mod signals;
mod suffix;
mod util;

const E_SUCCESS: i32 = 0;
const E_ERROR: i32 = 1;
const E_WARNING: i32 = 2;

// 定义 exit_status_type 枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)] // 确保与 C 的内存布局兼容
pub enum ExitStatusType {
    ESuccess = 0,
    EError = 1,
    EWarning = 2,
}

fn main() {
    println!("Hello, world!");
}
