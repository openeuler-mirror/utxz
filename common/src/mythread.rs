/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use libc::sigset_t;
use utxz_sys::signal as sys_signal;

#[derive(Default)]
pub struct MyThreadCondTime {
    /// 当前开始计时的刻度计数 (毫秒)。
    pub start: u32,
    /// 超时的时长(毫秒)。当当前计数减去 "start" 大于或等于 "timeout" 时超时。
    pub timeout: u32,
}

pub fn mythread_sigmask(how: i32, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) {
    // 通过 wrapper crate 集中承载 libc/FFI 的 unsafe
    let ret = sys_signal::sigprocmask(how, set, oset);
    assert!(ret.is_ok(), "sigprocmask failed: {:?}", ret.err());
}

pub fn get_tick_count() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    // 使用系统时间模拟 GetTickCount()，注意该实现仅为示例
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards");
    now.as_millis() as u32
}

// pub fn mythread_condtime_set(condtime: &mut MyThreadCondTime, _cond: &MyThreadCond, timeout: u32) {
//     condtime.start = get_tick_count();
//     condtime.timeout = timeout;
// }
