/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::{ptr, sync::Mutex};

use common::mythread_sigmask;
use lazy_static::lazy_static;
use libc::{sigaction, sighandler_t, sigset_t};
use utxz_sys::signal as sys_signal;

use crate::{
    file_io::io_write_to_user_abort_pipe,
    message::{message_signal_handler, MESSAGE_PROGRESS_SIGS},
};

lazy_static! {
    pub static ref USER_ABORT:Mutex<bool> = Mutex::new(false);
    pub static ref EXIT_SIGNAL:Mutex<i32> = Mutex::new(0);
    // pub static ref HOOKED_SIGNALS:Mutex<sigset_t> = Mutex::new(0);
    pub static ref HOOKED_SIGNALS: Mutex<sigset_t> = {
        Mutex::new(sys_signal::sigset_empty())
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

    // 构造 hooked signal set
    let mut set = sys_signal::sigset_empty();
    for sig in sigs {
        if sys_signal::sigaddset(&mut set, sig).is_err() {
            message_signal_handler();
        }
    }
    for &sig in MESSAGE_PROGRESS_SIGS {
        if sig != 0 && sys_signal::sigaddset(&mut set, sig).is_err() {
            message_signal_handler();
        }
    }
    *HOOKED_SIGNALS.lock().unwrap() = set;

    let mut my_sa: sigaction = sys_signal::zeroed_sigaction();
    my_sa.sa_mask = *HOOKED_SIGNALS.lock().unwrap();
    my_sa.sa_flags = 0;
    my_sa.sa_sigaction = signal_handler as sighandler_t;

    for sig in sigs {
        if let Ok(old) = sys_signal::sigaction_get(sig) {
            if old.sa_sigaction == libc::SIG_IGN {
                continue;
            }
        }

        if sys_signal::sigaction_set(sig, &my_sa).is_err() {
            message_signal_handler();
        }
    }

    *SIGNALS_ARE_INITIALIZED.lock().unwrap() = true;
}

pub fn signals_block() {
    if *SIGNALS_ARE_INITIALIZED.lock().unwrap() {
        *SIGNALS_BLOCK_COUNT.lock().unwrap() += 1;
        if *SIGNALS_BLOCK_COUNT.lock().unwrap() == 0 {
            mythread_sigmask(libc::SIG_BLOCK, Some(&HOOKED_SIGNALS.lock().unwrap()), None);
        }
    }
}

pub fn signals_unblock() {
    if *SIGNALS_ARE_INITIALIZED.lock().unwrap() {
        assert!(*SIGNALS_BLOCK_COUNT.lock().unwrap() > 0);
        if *SIGNALS_BLOCK_COUNT.lock().unwrap() == 1 {
            // 模拟解除信号阻塞
            let _saved_errno = std::io::Error::last_os_error();
            mythread_sigmask(
                libc::SIG_UNBLOCK,
                Some(&HOOKED_SIGNALS.lock().unwrap()),
                None,
            );
        }
        *SIGNALS_BLOCK_COUNT.lock().unwrap() -= 1;
    }
}

pub fn signals_exit() {
    let sig = *EXIT_SIGNAL.lock().unwrap();
    if sig != 0 {
        // Linux / Unix 处理方式
        let mut sa: sigaction = sys_signal::zeroed_sigaction();
        sa.sa_sigaction = libc::SIG_DFL;
        let _ = sys_signal::sigfillset(&mut sa.sa_mask);
        sa.sa_flags = 0;
        let _ = sys_signal::sigaction_set(sig, &sa);
        let _ = sys_signal::raise(sig);
    }
}
