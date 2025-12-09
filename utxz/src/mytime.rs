/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use lazy_static::lazy_static;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::coder::{OperationMode, OPT_MODE};

lazy_static! {
    pub static ref START_TIME: Mutex<u64> = Mutex::new(0);
    pub static ref NEXT_FLUSH: Mutex<u64> = Mutex::new(0);
    pub static ref OPT_FLUSH_TIMEOUT: Mutex<u64> = Mutex::new(0);
}

/// 获取当前时间（以毫秒为单位）
fn mytime_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::new(0, 0))
        .as_millis() as u64
}

/// 设置开始时间
pub fn mytime_set_start_time() {
    *START_TIME.lock().unwrap() = mytime_now();
}

/// 获取经过的时间（以毫秒为单位）
pub fn mytime_get_elapsed() -> u64 {
    let start_time = START_TIME.lock().unwrap();
    mytime_now() - *start_time
}

/// 设置刷新时间
pub fn mytime_set_flush_time() {
    *NEXT_FLUSH.lock().unwrap() = mytime_now() + *OPT_FLUSH_TIMEOUT.lock().unwrap();
}

/// 获取刷新超时时间
pub fn mytime_get_flush_timeout() -> i32 {
    if *OPT_FLUSH_TIMEOUT.lock().unwrap() == 0
        || *OPT_MODE.lock().unwrap() != OperationMode::Compress
    {
        return -1;
    }

    let now = mytime_now();
    if now > *NEXT_FLUSH.lock().unwrap() {
        return 0;
    }

    let remaining = *NEXT_FLUSH.lock().unwrap() - now;
    if remaining > i32::MAX as u64 {
        return i32::MAX;
    } else {
        return remaining as i32;
    }
}
