/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::{ptr, sync::Mutex};

use common::mythread_sigmask;
use lazy_static::lazy_static;
use libc::{raise, sigaction, sigaddset, sigemptyset, sigfillset, sighandler_t, sigset_t};
use nix::errno::errno;

use crate::{
    file_io::io_write_to_user_abort_pipe,
    message::{message_signal_handler, MESSAGE_PROGRESS_SIGS},
};

lazy_static! {
    pub static ref USER_ABORT:Mutex<bool> = Mutex::new(false);
    pub static ref EXIT_SIGNAL:Mutex<i32> = Mutex::new(0);
    // pub static ref HOOKED_SIGNALS:Mutex<sigset_t> = Mutex::new(0);
    pub static ref HOOKED_SIGNALS: Mutex<sigset_t> = {
        let mut sigset: sigset_t = unsafe { std::mem::zeroed() }; // 初始化为零
        unsafe { sigemptyset(&mut sigset) }; // 清空信号集
        Mutex::new(sigset)
    };

    pub static ref SIGNALS_ARE_INITIALIZED:Mutex<bool> = Mutex::new(false);
    pub static ref SIGNALS_BLOCK_COUNT:Mutex<usize> = Mutex::new(0);
}

fn signal_handler(sig: i32) {
    *EXIT_SIGNAL.lock().unwrap() = sig;
    *USER_ABORT.lock().unwrap() = true;
    io_write_to_user_abort_pipe();
}

pub fn signals_init() {
    let sigs = [
        libc::SIGINT,
        libc::SIGTERM,
        libc::SIGHUP,
        libc::SIGPIPE,
        libc::SIGXCPU,
        libc::SIGXFSZ,
    ];
    unsafe {
        sigemptyset(&mut *HOOKED_SIGNALS.lock().unwrap());
        for sig in sigs.iter() {
            sigaddset(&mut *HOOKED_SIGNALS.lock().unwrap(), *sig);
        }

        for i in MESSAGE_PROGRESS_SIGS {
            if *i != 0 {
                sigaddset(&mut *HOOKED_SIGNALS.lock().unwrap(), *i);
            }
        }

        let mut my_sa: sigaction = std::mem::zeroed();
        my_sa.sa_mask = *HOOKED_SIGNALS.lock().unwrap();
        my_sa.sa_flags = 0;
        my_sa.sa_sigaction = signal_handler as sighandler_t;

        for i in sigs.iter() {
            let mut old: sigaction = std::mem::zeroed();
            if sigaction(*i, std::ptr::null(), &mut old) == 0 && old.sa_sigaction == libc::SIG_IGN {
                continue;
            }
            if sigaction(*i, &my_sa, 0 as *mut sigaction) != 0 {
                message_signal_handler();
            }
        }

        *SIGNALS_ARE_INITIALIZED.lock().unwrap() = true;
    }
}

pub fn signals_block() {
    unsafe {
        if *SIGNALS_ARE_INITIALIZED.lock().unwrap() {
            *SIGNALS_BLOCK_COUNT.lock().unwrap() += 1;
            if *SIGNALS_BLOCK_COUNT.lock().unwrap() == 0 {
                mythread_sigmask(libc::SIG_BLOCK, Some(&HOOKED_SIGNALS.lock().unwrap()), None);
            }
        }
    }
}

pub fn signals_unblock() {
    unsafe {
        if *SIGNALS_ARE_INITIALIZED.lock().unwrap() {
            assert!(*SIGNALS_BLOCK_COUNT.lock().unwrap() > 0);
            if *SIGNALS_BLOCK_COUNT.lock().unwrap() == 1 {
                // 模拟解除信号阻塞
                let saved_errno = std::io::Error::last_os_error();
                mythread_sigmask(
                    libc::SIG_UNBLOCK,
                    Some(&HOOKED_SIGNALS.lock().unwrap()),
                    None,
                );
                // 恢复 errno
                //*libc::__errno_location() = saved_errno;
            }
            *SIGNALS_BLOCK_COUNT.lock().unwrap() -= 1;
        }
    }
}

pub fn signals_exit() {
    unsafe {
        let sig = *EXIT_SIGNAL.lock().unwrap();
        if sig != 0 {
            // 在这里我们模拟不同平台的信号处理行为

            {
                // Linux / Unix 处理方式
                let mut sa: sigaction = std::mem::zeroed();
                sa.sa_sigaction = libc::SIG_DFL;
                sigfillset(&mut sa.sa_mask);
                sa.sa_flags = 0;
                // 设置默认处理器
                libc::sigaction(sig, &sa, ptr::null_mut());
                // 发送信号
                raise(sig);
            }
        }
    }
}
