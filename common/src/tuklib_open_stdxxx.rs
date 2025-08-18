/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */


 #![warn(clippy::redundant_pattern_matching)]

use nix::fcntl::{fcntl, open, FcntlArg, OFlag};
use nix::sys::stat::Mode;
use nix::unistd::{close, dup2};
use std::process::exit;

#[no_mangle]
pub fn tuklib_open_stdxxx(err_status: i32) {
    for fd in 0..=2 {
        // 检查文件描述符是否打开
        match fcntl(fd, FcntlArg::F_GETFD) {
            Ok(_) => {} // 文件描述符已打开，继续下一个
            Err(nix::errno::Errno::EBADF) => {
                // 文件描述符未打开，尝试将其重定向到 /dev/null
                let flags = if fd == 0 {
                    OFlag::O_WRONLY | OFlag::O_NOCTTY
                } else {
                    OFlag::O_RDONLY | OFlag::O_NOCTTY
                };
                match open("/dev/null", flags, Mode::empty()) {
                    Ok(new_fd) => {
                        // 将 /dev/null 重定向到目标文件描述符
                        if dup2(new_fd, fd).is_err()  {
                            // 重定向失败，关闭新打开的文件描述符并退出
                            let _ = close(new_fd);
                            exit(err_status);
                        }
                        // 关闭原始的 /dev/null 文件描述符
                        let _ = close(new_fd);
                    }
                    Err(_) => {
                        // 打开 /dev/null 失败，退出程序
                        exit(err_status);
                    }
                }
            }
            Err(_) => {
                // 其他错误，退出程序
                exit(err_status);
            }
        }
    }
}
