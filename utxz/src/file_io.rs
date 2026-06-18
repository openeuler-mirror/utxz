/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![deny(clippy::bad_bit_mask)]
use common::tuklib_open_stdxxx;
use lazy_static::lazy_static;
use libc::{self, unlink, S_IFMT};
use libc::{
    c_char, close, fcntl, fstat, futimens, lseek, lstat, off_t, open, pipe, posix_fadvise, read,
    timespec, EAGAIN, EINTR, ELOOP, ENOENT, EPIPE, EWOULDBLOCK, F_GETFL, F_SETFL, O_APPEND,
    O_CREAT, O_EXCL, O_NOCTTY, O_NOFOLLOW, O_NONBLOCK, O_RDONLY, O_WRONLY, POSIX_FADV_RANDOM,
    POSIX_FADV_SEQUENTIAL, SEEK_CUR, SEEK_END, SEEK_SET, STDIN_FILENO, STDOUT_FILENO, S_IFDIR,
    S_IFREG, S_IRUSR, S_ISGID, S_ISUID, S_ISVTX, S_IWUSR,
};
use libc::{fchmod, fchown, stat, S_IFLNK};
// use nix::errno::Errno;
// use nix::poll::{poll, PollFd, PollFlags};
// use nix::sys::time::{TimeSpec, TimeValLike};
// use nix::unistd::unlink;
use libc::{poll, pollfd, POLLIN, POLLOUT};
use std::ffi::{c_void, CStr, CString};
use std::fs;
use std::fs::{metadata, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::{AsRawFd, RawFd};
use std::process::exit;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use utxz_sys::{fcntl as sys_fcntl, fs as sys_fs, poll as sys_poll, unistd as sys_unistd};

pub const IO_BUFFER_SIZE: usize = 262144;

use std::mem;

use crate::args::{OPT_FORCE, OPT_KEEP_ORIGINAL, OPT_STDOUT, STDIN_FILENAME};
use crate::coder::{OperationMode, OPT_MODE};
use crate::message::{message_bug, message_error, message_fatal, message_warning};
use crate::mytime::{mytime_get_flush_timeout, mytime_set_flush_time};
use crate::signals::{signals_block, signals_unblock, USER_ABORT};
use crate::suffix::suffix_get_dest_name;
use crate::E_ERROR;

#[derive(Debug, Clone, Copy)]
pub struct IoBuf {
    pub data: [u8; IO_BUFFER_SIZE],
}

impl IoBuf {
    // 创建一个新的 IoBuf，初始化为 0
    pub fn new() -> Self {
        IoBuf {
            data: [0; IO_BUFFER_SIZE],
        }
    }

    // 以 u8 数组的形式访问数据
    pub fn as_u8(&self) -> &[u8; IO_BUFFER_SIZE] {
        &self.data
    }

    // 以 u32 数组的形式访问数据
    pub fn as_u32(&self) -> &[u32; IO_BUFFER_SIZE / mem::size_of::<u32>()] {
        unsafe { &*(self.data.as_ptr() as *const [u32; IO_BUFFER_SIZE / mem::size_of::<u32>()]) }
    }

    // 以 u64 数组的形式访问数据
    pub fn as_u64(&self) -> &[u64; IO_BUFFER_SIZE / mem::size_of::<u64>()] {
        unsafe { &*(self.data.as_ptr() as *const [u64; IO_BUFFER_SIZE / mem::size_of::<u64>()]) }
    }

    // 以 u8 数组的形式访问可变数据
    pub fn as_u8_mut(&mut self) -> &mut [u8; IO_BUFFER_SIZE] {
        &mut self.data
    }

    // 以 u32 数组的形式访问可变数据
    pub fn as_u32_mut(&mut self) -> &mut [u32; IO_BUFFER_SIZE / mem::size_of::<u32>()] {
        unsafe {
            &mut *(self.data.as_mut_ptr() as *mut [u32; IO_BUFFER_SIZE / mem::size_of::<u32>()])
        }
    }

    // 以 u64 数组的形式访问可变数据
    pub fn as_u64_mut(&mut self) -> &mut [u64; IO_BUFFER_SIZE / mem::size_of::<u64>()] {
        unsafe {
            &mut *(self.data.as_mut_ptr() as *mut [u64; IO_BUFFER_SIZE / mem::size_of::<u64>()])
        }
    }
}

/// 文件对结构体
#[derive(Debug)]
pub struct FilePair {
    /// 源文件名（从命令行给出的文件名），
    /// 如果从标准输入读取，则指向静态字符串 "(stdin)"。
    pub src_name: Option<String>,

    /// 目标文件名，从 src_name 转换而来，
    /// 如果写入标准输出，则指向静态字符串 "(stdout)"。
    pub dest_name: Option<String>,

    /// 源文件的文件描述符
    pub src_fd: i32,

    /// 目标文件的文件描述符
    pub dest_fd: i32,

    /// 当检测到源文件已到达文件末尾时为 true
    pub src_eof: bool,

    /// 对于 --flush-timeout 选项：如果自上次刷新以来或自文件开始以来至少读取了一字节，则为 true
    pub src_has_seen_input: bool,

    /// 对于 --flush-timeout 选项：当需要刷新时为 true
    pub flush_needed: bool,

    /// 如果为 true，则我们尝试查找长的零字节块并创建稀疏文件
    pub dest_try_sparse: bool,

    /// 仅当 dest_try_sparse 为 true 时使用。它保存尚未写入的零字节数，
    /// 因为我们打算将该字节范围标记为稀疏块。
    pub dest_pending_sparse: i64,

    /// 源文件的状态（如大小、权限等）
    pub src_st: stat,

    /// 目标文件的状态（如大小、权限等）
    pub dest_st: stat,
}

impl FilePair {
    pub fn new(
        src_name: Option<&str>,
        dest_name: Option<&str>,
        src_fd: RawFd,
        dest_fd: RawFd,
    ) -> Self {
        // 获取源文件和目标文件的元数据
        fn get_stat(path: &str) -> stat {
            let c_path = CString::new(path).unwrap();
            sys_fs::stat(&c_path).expect("stat failed")
        }

        let src_st = src_name.map(get_stat).unwrap_or_else(sys_fs::zeroed_stat);
        let dest_st = dest_name.map(get_stat).unwrap_or_else(sys_fs::zeroed_stat);

        FilePair {
            src_name: src_name.map(|s| s.to_string()),
            dest_name: dest_name.map(|s| s.to_string()),
            src_fd,
            dest_fd,
            src_eof: false,
            src_has_seen_input: false,
            flush_needed: false,
            dest_try_sparse: false,
            dest_pending_sparse: 0,
            src_st,
            dest_st,
        }
    }
}

/// 用于表示 I/O 等待的结果
#[derive(Debug, PartialEq)]
enum IoWaitRet {
    IoWaitMore,    // 可以继续进行读取或写入
    IoWaitError,   // 错误或用户中断
    IoWaitTimeout, // poll() 超时
}

lazy_static! {
    /// 如果为 true，尝试在解压时创建稀疏文件
    static ref TRY_SPARSE: Mutex<bool> = Mutex::new(true);

    /// 标准输入的文件状态标志
    static ref STDIN_FLAGS: Mutex<i32> = Mutex::new(0);

    /// 是否恢复标准输入标志
    static ref RESTORE_STDIN_FLAGS: Mutex<bool> = Mutex::new(false);

    /// 标准输出的文件状态标志
    static ref STDOUT_FLAGS: Mutex<i32> = Mutex::new(0);

    /// 是否恢复标准输出标志
    static ref RESTORE_STDOUT_FLAGS: Mutex<bool> = Mutex::new(false);

    /// 用于用户中断的自管道
    static ref USER_ABORT_PIPE: Mutex<[RawFd; 2]> = Mutex::new([0, 0]);

    /// 如果为 true，显示文件所有权改变的警告
    static ref WARN_FCHOWN: Mutex<bool> = Mutex::new(false);
}

/// 初始化 I/O 相关设置
pub fn io_init() {
    // 打开标准输入、输出、错误流
    tuklib_open_stdxxx(E_ERROR);

    // 如果当前用户是 root，则在 fchown 失败时显示警告
    *WARN_FCHOWN.lock().unwrap() = sys_unistd::geteuid() == 0;

    // 创建用于用户中断的自管道
    let pipe_fds = match sys_unistd::pipe() {
        Ok(fds) => fds,
        Err(e) => {
            message_fatal(&format!("创建管道失败: {}", e), format_args!(""));
            unreachable!();
        }
    };

    // 将管道的两端设置为非阻塞模式
    for i in 0..2 {
        let flags = match sys_fcntl::fcntl_getfl(pipe_fds[i]) {
            Ok(v) => v,
            Err(e) => {
                message_fatal(&format!("设置管道非阻塞失败: {}", e), format_args!(""));
                unreachable!();
            }
        };
        if let Err(e) = sys_fcntl::fcntl_setfl(pipe_fds[i], flags | O_NONBLOCK) {
            message_fatal(&format!("设置管道非阻塞失败: {}", e), format_args!(""));
        }
    }

    // 更新全局变量
    *USER_ABORT_PIPE.lock().unwrap() = pipe_fds;
}

/// 向用户中断管道写入数据
pub fn io_write_to_user_abort_pipe() {
    let b: u8 = b'\0';
    let pipe_fds = USER_ABORT_PIPE.lock().unwrap();
    let _ = sys_unistd::write(pipe_fds[1], &[b]);
}

/// 禁用稀疏文件功能
pub fn io_no_sparse() {
    *TRY_SPARSE.lock().unwrap() = false;
}

/// 等待 I/O 操作完成
fn io_wait(pair: &FilePair, timeout: i32, is_reading: bool) -> IoWaitRet {
    let mut pfd = [
        pollfd {
            fd: if is_reading {
                pair.src_fd
            } else {
                pair.dest_fd
            },
            events: if is_reading { POLLIN } else { POLLOUT },
            revents: 0,
        },
        pollfd {
            fd: USER_ABORT_PIPE.lock().unwrap()[0],
            events: POLLIN,
            revents: 0,
        },
    ];

    loop {
        let ret = match sys_poll::poll(&mut pfd, timeout) {
            Ok(v) => v,
            Err(e) => {
                if e.kind() == io::ErrorKind::Interrupted || e.kind() == io::ErrorKind::WouldBlock {
                    continue;
                }

                message_error(
                    &format!(
                        "{}: poll() 失败: {}",
                        if is_reading {
                            pair.src_name.as_deref().unwrap_or("(stdin)")
                        } else {
                            pair.dest_name.as_deref().unwrap_or("(stdout)")
                        },
                        e
                    ),
                    format_args!(""),
                );
                return IoWaitRet::IoWaitError;
            }
        };

        if *USER_ABORT.lock().unwrap() {
            return IoWaitRet::IoWaitError;
        }

        if ret == 0 {
            return IoWaitRet::IoWaitTimeout; // 超时
        }

        if pfd[0].revents != 0 {
            return IoWaitRet::IoWaitMore; // 有事件发生
        }
    }
}

/// 删除文件
fn io_unlink(name: &str, known_st: &libc::stat) {
    // 获取当前文件元数据
    let stat_ret = if *OPT_FORCE.lock().unwrap() {
        fs::metadata(name)
    } else {
        fs::symlink_metadata(name)
    };
    match stat_ret {
        Ok(new_st) => {
            // 检查设备号和 inode 是否一致，避免误删
            if new_st.dev() != known_st.st_dev as u64 || new_st.ino() != known_st.st_ino as u64 {
                eprintln!("警告：{}: 文件似乎已被移动，未删除", name);
                return;
            }
            // 有竞争条件，但我们已尽力避免误删
            let c_name = CString::new(name).expect("CString::new failed");
            if let Err(e) = sys_fs::unlink(&c_name) {
                eprintln!("警告：{}: 无法删除: {}", name, e);
            }
        }
        Err(_) => {
            eprintln!("警告：{}: 文件似乎已被移动，未删除", name);
        }
    }
}

/// 拷贝文件属性
fn io_copy_attrs(pair: &FilePair) {
    // 设置文件所有者
    if sys_fs::fchown(pair.dest_fd, pair.src_st.st_uid, !(0 as libc::gid_t)).is_err()
        && *WARN_FCHOWN.lock().unwrap()
    {
        eprintln!(
            "警告：{}: 无法设置文件所有者: {}",
            pair.dest_name.as_deref().unwrap_or("(unknown)"),
            std::io::Error::last_os_error()
        );
    }

    let mut mode: u32;

    // 设置文件组
    if pair.dest_st.st_gid != pair.src_st.st_gid
        && sys_fs::fchown(pair.dest_fd, !(0 as libc::uid_t), pair.src_st.st_gid).is_err()
    {
        eprintln!(
            "警告：{}: 无法设置文件组: {}",
            pair.dest_name.as_deref().unwrap_or("(unknown)"),
            std::io::Error::last_os_error()
        );
        // 降级权限
        mode = ((pair.src_st.st_mode & 0o070) >> 3) & (pair.src_st.st_mode & 0o007);
        mode = (pair.src_st.st_mode & 0o700) | (mode << 3) | mode;
    } else {
        // 去除 setuid/setgid/sticky 位
        mode = pair.src_st.st_mode & 0o777;
    }

    // 设置权限
    if sys_fs::fchmod(pair.dest_fd, mode).is_err() {
        eprintln!(
            "警告：{}: 无法设置文件权限: {}",
            pair.dest_name.as_deref().unwrap_or("(unknown)"),
            std::io::Error::last_os_error()
        );
    }

    // 构造 timespec 结构体
    let ts = [
        libc::timespec {
            tv_sec: pair.src_st.st_atime,
            tv_nsec: pair.src_st.st_atime_nsec,
        },
        libc::timespec {
            tv_sec: pair.src_st.st_mtime,
            tv_nsec: pair.src_st.st_mtime_nsec,
        },
    ];

    let _ = sys_fs::futimens(pair.dest_fd, &ts);
}

pub fn s_isreg(mode: u32) -> bool {
    (mode & 0o170000) == 0o100000
}
/// 打开源文件，返回 true 表示出错，false 表示成功
fn io_open_src_real(pair: &mut FilePair) -> bool {
    // 如果读取的是标准输入
    if pair.src_name == Some(STDIN_FILENAME.to_string()) {
        pair.src_fd = STDIN_FILENO;

        match sys_fcntl::fcntl_getfl(STDIN_FILENO) {
            Ok(v) => *STDIN_FLAGS.lock().unwrap() = v,
            Err(e) => {
                eprintln!("错误：无法获取标准输入的文件状态标志: {}", e);
                return true;
            }
        }

        if (*STDIN_FLAGS.lock().unwrap() & O_NONBLOCK) == 0 {
            if sys_fcntl::fcntl_setfl(STDIN_FILENO, *STDIN_FLAGS.lock().unwrap() | O_NONBLOCK)
                .is_ok()
            {
                *RESTORE_STDIN_FLAGS.lock().unwrap() = true;
            }
        }

        // 忽略 posix_fadvise 的错误
        let _ = sys_fs::posix_fadvise(
            STDIN_FILENO,
            0,
            0,
            if *OPT_MODE.lock().unwrap() == OperationMode::List {
                POSIX_FADV_RANDOM
            } else {
                POSIX_FADV_SEQUENTIAL
            },
        );

        return false;
    }

    // 是否跟随符号链接
    let follow_symlinks = *OPT_STDOUT.lock().unwrap()
        || *OPT_FORCE.lock().unwrap()
        || *OPT_KEEP_ORIGINAL.lock().unwrap();
    let reg_files_only = !*OPT_STDOUT.lock().unwrap();

    // open() 标志
    let mut flags = O_RDONLY | 0 | O_NOCTTY;
    flags |= O_NONBLOCK;
    if !follow_symlinks {
        flags |= O_NOFOLLOW;
    }

    // 打开文件
    let c_name = match &pair.src_name {
        Some(name) => CString::new(name.as_str()).unwrap(),
        None => panic!("源文件名为空"),
    };

    match sys_fcntl::open_with_mode(&c_name, flags, 0) {
        Ok(fd) => pair.src_fd = fd,
        Err(err) => {
            // EINTR 不应出现
            let errno = err.raw_os_error().unwrap_or(0);
            assert_ne!(errno, EINTR);

            let mut was_symlink = false;
            if errno == ELOOP && !follow_symlinks {
                let mut st: stat = sys_fs::zeroed_stat();
                if sys_fs::lstat(&c_name, &mut st).is_ok() && (st.st_mode & S_IFMT) == S_IFLNK {
                    was_symlink = true;
                }
            }

            if was_symlink {
                message_warning(
                    &format!(
                        "{}: Is a symbolic link, skipping",
                        pair.src_name.as_deref().unwrap_or("(unknown)")
                    ),
                    &[],
                );
            } else {
                message_error(
                    &format!(
                        "{}: {}",
                        pair.src_name.as_deref().unwrap_or("(unknown)"),
                        err
                    ),
                    format_args!(""),
                );
            }
            return true;
        }
    }

    // 获取文件状态
    if let Err(err) = sys_fs::fstat(pair.src_fd, &mut pair.src_st) {
        message_error(
            &format!(
                "{}: {}",
                pair.src_name.as_deref().unwrap_or("(unknown)"),
                err
            ),
            format_args!(""),
        );
        let _ = sys_unistd::close(pair.src_fd);
        return true;
    }

    // 检查目录
    if (pair.src_st.st_mode & S_IFMT) == S_IFDIR {
        message_warning(
            &format!(
                "{}: Is a directory, skipping",
                pair.src_name.as_deref().unwrap_or("(unknown)")
            ),
            &[],
        );
        let _ = sys_unistd::close(pair.src_fd);
        return true;
    }

    // 只允许常规文件
    if reg_files_only && !((pair.src_st.st_mode & S_IFMT) == S_IFREG) {
        message_warning(
            &format!(
                "{}: Not a regular file, skipping",
                pair.src_name.as_deref().unwrap_or("(unknown)")
            ),
            &[],
        );
        let _ = sys_unistd::close(pair.src_fd);
        return true;
    }

    // 检查特殊权限和硬链接数
    if reg_files_only && !*OPT_FORCE.lock().unwrap() && !*OPT_KEEP_ORIGINAL.lock().unwrap() {
        let mode = pair.src_st.st_mode;
        if (mode & S_ISUID as u32 != 0) || (mode & S_ISGID as u32 != 0) {
            message_warning(
                &format!(
                    "{}: File has setuid or setgid bit set, skipping",
                    pair.src_name.as_deref().unwrap_or("(unknown)")
                ),
                &[],
            );
            let _ = sys_unistd::close(pair.src_fd);
            return true;
        }
        if mode & S_ISVTX as u32 != 0 {
            message_warning(
                &format!(
                    "{}: File has sticky bit set, skipping",
                    pair.src_name.as_deref().unwrap_or("(unknown)")
                ),
                &[],
            );
            let _ = sys_unistd::close(pair.src_fd);
            return true;
        }
        if pair.src_st.st_nlink > 1 {
            message_warning(
                &format!(
                    "{}: Input file has more than one hard link, skipping",
                    pair.src_name.as_deref().unwrap_or("(unknown)")
                ),
                &[],
            );
            let _ = sys_unistd::close(pair.src_fd);
            return true;
        }
    }

    // 不是常规文件时等待 IO
    if !s_isreg(pair.src_st.st_mode as u32) {
        signals_unblock();
        let ret = io_wait(pair, -1, true);
        signals_block();

        if ret != IoWaitRet::IoWaitMore {
            let _ = sys_unistd::close(pair.src_fd);
            return true;
        }
    }

    // 忽略 posix_fadvise 的错误
    let _ = sys_fs::posix_fadvise(
        pair.src_fd,
        0,
        0,
        if *OPT_MODE.lock().unwrap() == OperationMode::List {
            POSIX_FADV_RANDOM
        } else {
            POSIX_FADV_SEQUENTIAL
        },
    );

    false
}

/// 打开源文件，返回 Some(FilePair) 表示成功，None 表示失败
pub fn io_open_src(src_name: &str) -> Option<FilePair> {
    if src_name.is_empty() {
        message_error("(empty filename)", format_args!(""));
        return None;
    }

    // 静态分配结构体（Rust中用局部变量即可）
    let mut pair = FilePair {
        src_name: Some(src_name.to_string()),
        dest_name: None,
        src_fd: -1,
        dest_fd: -1,
        src_eof: false,
        src_has_seen_input: false,
        flush_needed: false,
        dest_try_sparse: false,
        dest_pending_sparse: 0,
        src_st: sys_fs::zeroed_stat(),
        dest_st: sys_fs::zeroed_stat(),
    };

    // 阻塞信号
    signals_block();
    let error: bool = io_open_src_real(&mut pair);
    signals_unblock();

    if error {
        None
    } else {
        Some(pair)
    }
}

/// 关闭源文件
fn io_close_src(pair: &mut FilePair, success: bool) {
    // 恢复标准输入标志
    if *RESTORE_STDIN_FLAGS.lock().unwrap() {
        assert!(pair.src_fd == STDIN_FILENO);
        *RESTORE_STDIN_FLAGS.lock().unwrap() = false;
        if let Err(e) = sys_fcntl::fcntl_setfl(STDIN_FILENO, *STDIN_FLAGS.lock().unwrap()) {
            eprintln!("错误：恢复标准输入状态标志失败: {}", e);
        }
    }

    if pair.src_fd != STDIN_FILENO && pair.src_fd != -1 {
        // 先关闭文件再考虑删除
        let _ = sys_unistd::close(pair.src_fd);

        if success && !*OPT_KEEP_ORIGINAL.lock().unwrap() {
            io_unlink(
                &<Option<std::string::String> as Clone>::clone(&pair.src_name).unwrap(),
                &pair.src_st,
            );
        }
    }
}

/// 打开目标文件，返回 true 表示出错，false 表示成功
fn io_open_dest_real(pair: &mut FilePair) -> bool {
    if *OPT_STDOUT.lock().unwrap() || pair.src_fd == STDIN_FILENO {
        // 输出到标准输出
        pair.dest_name = Some("(stdout)".to_string());
        pair.dest_fd = STDOUT_FILENO;

        match sys_fcntl::fcntl_getfl(STDOUT_FILENO) {
            Ok(v) => *STDOUT_FLAGS.lock().unwrap() = v,
            Err(e) => {
                eprintln!("错误：获取标准输出文件状态标志失败: {}", e);
                return true;
            }
        }

        if (*STDOUT_FLAGS.lock().unwrap() & O_NONBLOCK) == 0 {
            if sys_fcntl::fcntl_setfl(STDOUT_FILENO, *STDOUT_FLAGS.lock().unwrap() | O_NONBLOCK)
                .is_ok()
            {
                *RESTORE_STDOUT_FLAGS.lock().unwrap() = true;
            }
        }
    } else {
        // 获取目标文件名
        pair.dest_name = suffix_get_dest_name(pair.src_name.clone().unwrap().as_str());
        if pair.dest_name.is_none() {
            return true;
        }

        // --force 先尝试删除目标文件
        if *OPT_FORCE.lock().unwrap() {
            let c_dest = CString::new(pair.dest_name.as_ref().unwrap().as_str()).unwrap();
            if let Err(e) = sys_fs::unlink(&c_dest) {
                if e.raw_os_error().unwrap_or(0) != ENOENT {
                    eprintln!(
                        "错误：{}: 无法删除: {}",
                        pair.dest_name.as_ref().unwrap(),
                        e
                    );
                    return true;
                }
            }
        }

        // 打开目标文件
        let flags = O_WRONLY | 0 | O_NOCTTY | O_CREAT | O_EXCL | O_NONBLOCK;
        let mode = (S_IRUSR | S_IWUSR) as libc::mode_t;
        let c_dest = CString::new(pair.dest_name.as_ref().unwrap().as_str()).unwrap();
        match sys_fcntl::open_with_mode(&c_dest, flags, mode) {
            Ok(fd) => pair.dest_fd = fd,
            Err(e) => {
                message_error(
                    &format!("{}: {}", pair.dest_name.as_ref().unwrap(), e),
                    format_args!(""),
                );
                return true;
            }
        }
    }

    // 获取目标文件状态
    if sys_fs::fstat(pair.dest_fd, &mut pair.dest_st).is_err() {
        // fstat 失败，安全降级
        pair.dest_st.st_dev = 0;
        pair.dest_st.st_ino = 0;
    } else if *TRY_SPARSE.lock().unwrap() && *OPT_MODE.lock().unwrap() == OperationMode::Decompress
    {
        // 稀疏文件处理
        if pair.dest_fd == STDOUT_FILENO {
            if S_IFREG != pair.dest_st.st_mode {
                return false;
            }
            if *STDOUT_FLAGS.lock().unwrap() & O_APPEND != 0 {
                if sys_fs::lseek(STDOUT_FILENO, 0, SEEK_END).is_err() {
                    return false;
                }
                let mut flags = *STDOUT_FLAGS.lock().unwrap() & !O_APPEND;
                if *RESTORE_STDOUT_FLAGS.lock().unwrap() {
                    flags |= O_NONBLOCK;
                }
                if sys_fcntl::fcntl_setfl(STDOUT_FILENO, flags).is_err() {
                    return false;
                }
                *RESTORE_STDOUT_FLAGS.lock().unwrap() = true;
            } else if sys_fs::lseek(STDOUT_FILENO, 0, SEEK_CUR).ok() != Some(pair.dest_st.st_size) {
                return false;
            }
        }
        pair.dest_try_sparse = true;
    }

    false
}

/// 打开目标文件的包装函数
pub fn io_open_dest(pair: &mut FilePair) -> bool {
    signals_block();
    let ret = io_open_dest_real(pair);
    signals_unblock();
    ret
}

/// 关闭目标文件，success 为 false 时会删除目标文件
fn io_close_dest(pair: &mut FilePair, success: bool) -> bool {
    // 如果 io_open_dest() 禁用了 O_APPEND，这里恢复
    if *RESTORE_STDOUT_FLAGS.lock().unwrap() {
        assert!(pair.dest_fd == STDOUT_FILENO);
        *RESTORE_STDOUT_FLAGS.lock().unwrap() = false;
        if let Err(e) = sys_fcntl::fcntl_setfl(STDOUT_FILENO, *STDOUT_FLAGS.lock().unwrap()) {
            eprintln!("错误：恢复标准输出 O_APPEND 标志失败: {}", e);
            return true;
        }
    }

    if pair.dest_fd == -1 || pair.dest_fd == STDOUT_FILENO {
        return false;
    }

    if let Err(e) = sys_unistd::close(pair.dest_fd) {
        eprintln!("错误：关闭文件失败: {}", e);
        // 关闭失败，不能信任文件内容，删除之
        if let Some(ref name) = pair.dest_name {
            io_unlink(name, &pair.dest_st);
        }
        return true;
    }

    // 如果操作未成功，删除目标文件
    if !success {
        if let Some(ref name) = pair.dest_name {
            io_unlink(name, &pair.dest_st);
        }
    }

    false
}

/// 关闭文件对，包括处理稀疏文件、拷贝属性、关闭目标和源文件
pub fn io_close(pair: &mut FilePair, mut success: bool) {
    // 处理稀疏文件结尾
    if success && pair.dest_try_sparse && pair.dest_pending_sparse > 0 {
        // 向前 seek 到空洞末尾，写一个 0 字节
        if sys_fs::lseek(pair.dest_fd, pair.dest_pending_sparse - 1, SEEK_CUR).is_err() {
            eprintln!(
                "错误：创建稀疏文件时 seek 失败: {}",
                std::io::Error::last_os_error()
            );
            success = false;
        } else {
            let zero = [0u8];
            if io_write_buf(pair, &zero, 1) {
                success = false;
            }
        }
    }

    signals_block();

    // 拷贝文件属性（仅当目标文件已打开且不是标准输出）
    if success && pair.dest_fd != -1 && pair.dest_fd != STDOUT_FILENO {
        io_copy_attrs(pair);
    }

    // 先关闭目标文件，失败则不删除源文件
    if io_close_dest(pair, success) {
        success = false;
    }

    // 关闭源文件，如操作成功且未请求保留源文件则删除源文件
    io_close_src(pair, success);

    signals_unblock();
}

pub fn io_fix_src_pos(pair: &mut FilePair, rewind: usize) {
    assert!(rewind <= IO_BUFFER_SIZE);

    if rewind > 0 {
        // 对于不可 seek 的 fd 忽略错误
        let _ = sys_fs::lseek(pair.src_fd, -(rewind as off_t), SEEK_CUR);
    }
}

/// 从源文件读取数据，返回实际读取字节数
pub fn io_read(pair: &mut FilePair, buf: &mut IoBuf, size: usize) -> usize {
    assert!(size < usize::MAX);

    let mut pos = 0;

    while pos < size {
        match sys_unistd::read(pair.src_fd, &mut buf.data[pos..size]) {
            Ok(0) => {
                pair.src_eof = true;
                break;
            }
            Ok(amount) => {
                pos += amount;
                if !pair.src_has_seen_input {
                    pair.src_has_seen_input = true;
                    mytime_set_flush_time();
                }
            }
            Err(err) => {
                let errno = err.raw_os_error().unwrap_or(0);
                if errno == EINTR {
                    if *USER_ABORT.lock().unwrap() {
                        return usize::MAX;
                    }
                    continue;
                }

                if errno == EAGAIN || errno == EWOULDBLOCK {
                    let timeout = if pair.src_has_seen_input {
                        mytime_get_flush_timeout()
                    } else {
                        -1
                    };

                    match io_wait(pair, timeout, true) {
                        IoWaitRet::IoWaitMore => continue,
                        IoWaitRet::IoWaitError => return usize::MAX,
                        IoWaitRet::IoWaitTimeout => {
                            pair.flush_needed = true;
                            return pos;
                        }
                    }
                }

                message_error(
                    &format!(
                        "{}: 读取错误: {}",
                        pair.src_name.as_deref().unwrap_or("(unknown)"),
                        err
                    ),
                    format_args!(""),
                );
                return usize::MAX;
            }
        }
    }

    pos
}

/// 源文件定位到指定位置
pub fn io_seek_src(pair: &mut FilePair, pos: u64) -> bool {
    // 不允许 seek 到文件末尾之后
    if pos > pair.src_st.st_size as u64 {
        message_bug();
    }

    let ret = sys_fs::lseek(pair.src_fd, pos as off_t, SEEK_SET);
    if let Err(e) = ret {
        message_error(
            &format!("{:#?}: 定位文件出错: {}", pair.src_name, e),
            format_args!(""),
        );
        return true;
    }

    pair.src_eof = false;
    false
}

/// 从指定位置读取数据到缓冲区，返回 true 表示出错，false 表示成功
pub fn io_pread(pair: &mut FilePair, buf: &mut IoBuf, size: usize, pos: u64) -> bool {
    // 先 seek 到指定位置
    if io_seek_src(pair, pos) {
        return true;
    }

    // 读取数据
    let amount = io_read(pair, buf, size);
    if amount == usize::MAX {
        return true;
    }

    // 如果未读满，报错
    if amount != size {
        message_error(
            &format!("{:#?}: 文件意外结束", pair.src_name),
            format_args!(""),
        );
        return true;
    }

    false
}
/// 判断缓冲区是否全为0（稀疏块）
fn is_sparse(buf: &[u8]) -> bool {
    let word_size = size_of::<u64>();
    assert!(buf.len() % word_size == 0);

    let words = buf.len() / word_size;
    let u64_ptr = buf.as_ptr() as *const u64;
    for i in 0..words {
        unsafe {
            if *u64_ptr.add(i) != 0 {
                return false;
            }
        }
    }

    true
}

/// 写缓冲区到目标文件，返回 true 表示出错
fn io_write_buf(pair: &mut FilePair, buf: &[u8], mut size: usize) -> bool {
    assert!(size < usize::MAX);

    let mut remaining = &buf[..size];
    while !remaining.is_empty() {
        match sys_unistd::write(pair.dest_fd, remaining) {
            Ok(amount) => {
                remaining = &remaining[amount..];
            }
            Err(err) => {
                let errno = err.raw_os_error().unwrap_or(0);
                if errno == EINTR {
                    continue;
                }
                if errno == EAGAIN || errno == EWOULDBLOCK {
                    if io_wait(pair, -1, false) == IoWaitRet::IoWaitMore {
                        continue;
                    }
                    return true;
                }
                if errno != EPIPE {
                    message_error(
                        &format!(
                            "{}: 写入错误: {}",
                            pair.dest_name.as_deref().unwrap_or("(unknown)"),
                            err
                        ),
                        format_args!(""),
                    );
                }
                return true;
            }
        }
    }
    false
}

/// 写数据到目标文件，支持稀疏文件优化
pub fn io_write(pair: &mut FilePair, buf: &[u8], size: usize) -> bool {
    assert!(size <= IO_BUFFER_SIZE);
    let buf = &buf[..size];

    if pair.dest_try_sparse {
        // 检查是否为稀疏块（全为0），如果是则只记录空洞长度
        if size == IO_BUFFER_SIZE {
            let pending_max: off_t = 1 << (8 * size_of::<off_t>() - 2);
            if is_sparse(buf) && pair.dest_pending_sparse < pending_max {
                pair.dest_pending_sparse += size as off_t;
                return false;
            }
        } else if size == 0 {
            return false;
        }

        // 非稀疏块，如果有待处理的空洞，先跳过
        if pair.dest_pending_sparse > 0 {
            let seek_ret = sys_fs::lseek(pair.dest_fd, pair.dest_pending_sparse, SEEK_CUR);
            if let Err(e) = seek_ret {
                message_error(
                    &format!(
                        "{}: 创建稀疏文件时 seek 失败: {}",
                        pair.dest_name.as_deref().unwrap_or("(unknown)"),
                        e
                    ),
                    format_args!(""),
                );
                return true;
            }
            pair.dest_pending_sparse = 0;
        }
    }

    io_write_buf(pair, buf, size)
}
