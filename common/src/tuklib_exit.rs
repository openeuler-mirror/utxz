/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::common::get_progname;
use std::io::{self, stderr, stdout, Write};
use std::process::exit;

// tuklib_exit 函数
#[no_mangle]
pub fn tuklib_exit(mut status: i32, err_status: i32, show_error: i32) {
    // 获取程序名称
    let progname = get_progname().unwrap();

    if status != err_status {
        // 尝试关闭 stdout
        if let Err(e) = stdout().flush() {
            let ferror_err = e.kind() == io::ErrorKind::Other;
            let fclose_err = e.kind() == io::ErrorKind::Other;

            if ferror_err || fclose_err {
                if show_error != 0 {
                    let error_message = format!(
                        "{}: {}: {}\n",
                        progname,
                        "Writing to standard output failed",
                        if fclose_err {
                            e.to_string()
                        } else {
                            "Unknown error".to_string()
                        }
                    );
                    let _ = stderr().write_all(error_message.as_bytes());
                }
                status = err_status;
            }
        }
    }

    if status != err_status {
        // 尝试关闭 stderr
        if let Err(e) = stderr().flush() {
            let ferror_err = e.kind() == io::ErrorKind::Other;
            let fclose_err = e.kind() == io::ErrorKind::Other;

            if ferror_err || fclose_err {
                status = err_status;
            }
        }
    }

    // 退出程序
    exit(status);
}
