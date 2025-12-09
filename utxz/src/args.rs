/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use lazy_static::lazy_static;
use std::error::Error;
use std::str;
use std::sync::Mutex;

lazy_static! {
    pub static ref OPT_STDOUT: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_FORCE: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_KEEP_ORIGINAL: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_ROBOT: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_IGNORE_CHECK: Mutex<bool> = Mutex::new(false);
}
