/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use libc::{sigprocmask, sigset_t};
use std::ptr;

#[derive(Default)]
pub struct MyThreadCondTime {
    /// 当前开始计时的刻度计数 (毫秒)。
    pub start: u32,
    /// 超时的时长(毫秒)。当当前计数减去 "start" 大于或等于 "timeout" 时超时。
    pub timeout: u32,
}

pub fn mythread_sigmask(how: i32, set: Option<&sigset_t>, oset: Option<&mut sigset_t>) {
    // 调用 sigprocmask 来修改信号掩码
    let ret = unsafe {
        sigprocmask(
            how,
            set.map_or(ptr::null(), |s| s),
            oset.map_or(ptr::null_mut(), |os| os),
        )
    };

    // 检查返回值是否为 0 (表示操作成功)
    assert!(ret == 0, "sigprocmask failed with error code: {}", ret);
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
