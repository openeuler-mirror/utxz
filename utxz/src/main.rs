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

use file_io::io_init;
use hardware::hardware_init;
use lazy_static::lazy_static;
use signals::{signals_exit, signals_init, USER_ABORT};
use std::{
    env,
    io::{self, Read},
    sync::{Mutex, Once},
    thread,
    time::Duration,
};

const E_SUCCESS: i32 = 0;
const E_ERROR: i32 = 1;
const E_WARNING: i32 = 2;

// 使用 lazy_static 来创建全局静态变量
lazy_static! {
    static ref EXIT_STATUS: Mutex<ExitStatusType> = Mutex::new(ExitStatusType::ESuccess);
    static ref NO_WARN: Mutex<bool> = Mutex::new(false);
}

// 定义 exit_status_type 枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)] // 确保与 C 的内存布局兼容
pub enum ExitStatusType {
    ESuccess = 0,
    EError = 1,
    EWarning = 2,
}

pub fn set_exit_status(new_status: ExitStatusType) {
    // 确保 new_status 为 E_WARNING 或 E_ERROR
    assert!(new_status == ExitStatusType::EWarning || new_status == ExitStatusType::EError);

    // 更新 exit_status（如果不等于 E_ERROR）
    if *EXIT_STATUS.lock().unwrap() != ExitStatusType::EError {
        *EXIT_STATUS.lock().unwrap() = new_status;
    }
}

fn main() {
    println!("Hello, world!");
}
