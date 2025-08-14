/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

// 定义一个全局变量来存储程序名称
lazy_static::lazy_static! {
    pub static ref PROGNAME: std::sync::Mutex< String > = std::sync::Mutex::new( "".to_string());
}

// 设置程序名称
pub fn set_progname(progname: &str) {
    let mut progname_lock = PROGNAME.lock().unwrap();
    *progname_lock = progname.to_string();
}

// 获取程序名称
pub fn get_progname() -> Option<String> {
    let progname_lock = PROGNAME.lock().unwrap();
    Some(progname_lock.clone())
}
