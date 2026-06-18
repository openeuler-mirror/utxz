/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![warn(unused_must_use)]
use common::{tuklib_exit, PROGNAME};
use lazy_static::lazy_static;
use libc::ENOMEM;
use liblzma::api::{LzmaFilter, LzmaRet, LzmaStream, LZMA_VERSION};
use liblzma::common::{lzma_get_progress, lzma_version_number, lzma_version_string};
use signal_hook::consts::signal::SIGALRM;
use signal_hook::iterator::Signals;
use std::io::{self, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

// 或者
use crate::args::OPT_STDOUT;
use crate::coder::{OperationMode, OPT_MODE};
use crate::hardware::hardware_memlimit_get;
use crate::mytime::mytime_get_elapsed;
use crate::signals::{signals_block, signals_unblock};
use crate::util::{round_up_to_mib, uint64_to_str};
use crate::{set_exit_status, ExitStatusType, E_ERROR};
use signal_hook::consts::signal::SIGUSR1;
use utxz_sys::unistd as sys_unistd;

/// 输出详细程度
#[derive(PartialEq, PartialOrd, Clone)]
pub enum MessageVerbosity {
    Silent,  // 不输出任何信息
    Error,   // 仅输出错误信息
    Warning, // 输出错误和警告信息
    Verbose, // 输出错误、警告和详细统计信息
    Debug,   // 输出非常详细的信息
}

pub const MESSAGE_PROGRESS_SIGS: &[i32] = &[
    SIGALRM, SIGUSR1, 0, // 哨兵值，表示数组结束
];

// static mut PROGRESS_STRM: *mut LzmaStream = std::ptr::null_mut();

// 使用Rust风格的线程安全变量替代C风格的全局变量
// 线程安全的指针包装器
struct ThreadSafePtr(*mut LzmaStream<'static>);
unsafe impl Send for ThreadSafePtr {}
unsafe impl Sync for ThreadSafePtr {}

lazy_static! {
    static ref PROGRESS_STRM: Mutex<ThreadSafePtr> = Mutex::new(ThreadSafePtr(std::ptr::null_mut()));
    static ref FILES_POS: Mutex<u32> = Mutex::new(0);
    static ref FILES_TOTAL: Mutex< u32 > = Mutex::new(0);
    static ref VERBOSITY: Mutex<MessageVerbosity> = Mutex::new(MessageVerbosity::Warning);
    static ref FILENAME: Mutex<Option<String>> = Mutex::new(None);
    static ref FIRST_FILENAME_PRINTED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    static ref CURRENT_FILENAME_PRINTED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    static ref PROGRESS_AUTOMATIC: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    static ref PROGRESS_STARTED: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    static ref PROGRESS_ACTIVE: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    // static ref PROGRESS_STREAM: Mutex<Option<&'static mut LzmaStream>> = Mutex::new(None);
    static ref PROGRESS_IS_FROM_PASSTHRU: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    static ref EXPECTED_IN_SIZE: Mutex<u64> = Mutex::new(0);
}

static PROGRESS_NEEDS_UPDATING: AtomicBool = AtomicBool::new(false);

// 信号处理函数
fn progress_signal_handler() {
    // 设置 `PROGRESS_NEEDS_UPDATING` 为 true
    PROGRESS_NEEDS_UPDATING.store(true, Ordering::SeqCst);
}

// 初始化消息系统
pub fn message_init() {
    // 如果 --verbose 被使用，则仅当 stderr 是终端时使用进度指示器。
    // 如果 stderr 不是终端，则仅在完成文件后输出详细信息。
    // 特殊情况下，即使未使用 --verbose，用户也可以通过发送 SIGALRM 来打印进度信息一次，而无需自动更新。
    let progress_automatic = atty::is(atty::Stream::Stderr);

    // 注册信号处理函数
    let mut signals = Signals::new(&[SIGALRM]).expect("Failed to register signal handler");
    thread::spawn(move || {
        for _ in signals.forever() {
            progress_signal_handler();
        }
    });

    // 设置全局变量
    PROGRESS_AUTOMATIC.store(progress_automatic, Ordering::SeqCst);
}

pub fn message_verbosity_increase() {
    let mut verbosity = VERBOSITY.lock().unwrap();
    *verbosity = match *verbosity {
        MessageVerbosity::Silent => MessageVerbosity::Error,
        MessageVerbosity::Error => MessageVerbosity::Warning,
        MessageVerbosity::Warning => MessageVerbosity::Verbose,
        MessageVerbosity::Verbose => MessageVerbosity::Debug,
        MessageVerbosity::Debug => MessageVerbosity::Debug, // 已达最大值，不再增加
    };
}

pub fn message_verbosity_decrease() {
    let mut verbosity = VERBOSITY.lock().unwrap();
    if *verbosity > MessageVerbosity::Silent {
        *verbosity = match *verbosity {
            MessageVerbosity::Debug => MessageVerbosity::Verbose,
            MessageVerbosity::Verbose => MessageVerbosity::Warning,
            MessageVerbosity::Warning => MessageVerbosity::Error,
            MessageVerbosity::Error => MessageVerbosity::Silent,
            _ => MessageVerbosity::Silent,
        };
    }
}

pub fn message_verbosity_get() -> MessageVerbosity {
    (*VERBOSITY.lock().unwrap()).clone()
}

pub fn message_set_files(files: u32) {
    *FILES_TOTAL.lock().unwrap() = files;
}

/// Prints the name of the current file if it hasn't been printed already,
/// except if we are processing exactly one stream from stdin to stdout.
/// I think it looks nicer to not print "(stdin)" when --verbose is used
/// in a pipe and no other files are processed.
pub fn print_filename() {
    let opt_robot = *OPT_STDOUT.lock().unwrap(); // 假设 opt_robot 是一个布尔值
    let stdin_filename = "(stdin)".to_string(); // 假设 stdin_filename 是一个字符串

    if !opt_robot
        && (*FILES_TOTAL.lock().unwrap() != 1 || *FILENAME.lock().unwrap() != Some(stdin_filename))
    {
        // 信号阻塞逻辑
        signals_block();
        let opt_mode = OPT_MODE.lock().unwrap();
        // let file = if opt_mode == OperationMode::List { io::stdout() } else { io::stderr() };
        let mut file: Box<dyn Write + Send> = if *opt_mode == OperationMode::List {
            Box::new(io::stdout())
        } else {
            Box::new(io::stderr())
        };
        // 如果已经处理过文件，则在下一个文件名前添加空行以提高可读性
        if FIRST_FILENAME_PRINTED.load(Ordering::SeqCst) {
            writeln!(file, "").unwrap();
        }

        FIRST_FILENAME_PRINTED.store(true, Ordering::SeqCst);
        CURRENT_FILENAME_PRINTED.store(true, Ordering::SeqCst);

        // 如果不知道文件总数，因为使用了 --files 或 --files0
        if *FILES_TOTAL.lock().unwrap() == 0 {
            writeln!(
                file,
                "{} ({})",
                FILENAME.lock().unwrap().as_ref().unwrap(),
                FILES_POS.lock().unwrap()
            )
            .unwrap();
        } else {
            writeln!(
                file,
                "{} ({}/{})",
                FILENAME.lock().unwrap().as_ref().unwrap(),
                FILES_POS.lock().unwrap(),
                FILES_TOTAL.lock().unwrap()
            )
            .unwrap();
        }

        // 信号解除阻塞逻辑
        signals_unblock();
    }
}

/// 设置当前文件名
pub fn message_filename(src_name: &str) {
    // 存储文件名
    *FILENAME.lock().unwrap() = Some(src_name.to_string());

    // 从1开始编号文件
    // let mut files_pos = FILES_POS.lock().unwrap();
    *FILES_POS.lock().unwrap() += 1;

    // 如果详细程度达到V_VERBOSE或者处于MODE_LIST模式，则打印文件名
    if message_verbosity_get() >= MessageVerbosity::Verbose
        && (PROGRESS_AUTOMATIC.load(Ordering::SeqCst)
            || *OPT_MODE.lock().unwrap() == OperationMode::List)
    {
        print_filename();
    } else {
        CURRENT_FILENAME_PRINTED.store(false, Ordering::SeqCst);
    }
}

/// 开始进度指示器
pub fn message_progress_start(strm: &mut LzmaStream, is_passthru: bool, in_size: u64) {
    // 存储用于编码/解码的lzma_stream指针
    *PROGRESS_STRM.lock().unwrap() =
        ThreadSafePtr(unsafe { std::mem::transmute(strm as *mut LzmaStream) });

    PROGRESS_IS_FROM_PASSTHRU.store(is_passthru, Ordering::SeqCst);

    // 存储文件的预期大小
    *EXPECTED_IN_SIZE.lock().unwrap() = in_size;

    // 表示在打印错误消息之前可能需要打印进度信息
    PROGRESS_STARTED.store(true, Ordering::SeqCst);

    // 如果需要进度指示器，现在打印文件名和可能的文件计数
    if message_verbosity_get() >= MessageVerbosity::Verbose
        && PROGRESS_AUTOMATIC.load(Ordering::SeqCst)
    {
        // 延迟1秒后显示第一个进度信息
        #[cfg(unix)]
        {
            sys_unistd::alarm(0);
            PROGRESS_NEEDS_UPDATING.store(false, Ordering::SeqCst);
            sys_unistd::alarm(1);
        }
        #[cfg(not(unix))]
        {
            PROGRESS_NEEDS_UPDATING.store(true, Ordering::SeqCst);
            // 这里可以设置一个替代的超时机制
        }
    }
}

/// 计算并返回进度百分比字符串
fn progress_percentage(in_pos: u64) -> String {
    // 如果输入文件的大小未知，或者已处理的数据量超过了预期大小，返回 "--- %"
    if *EXPECTED_IN_SIZE.lock().unwrap() == 0 || in_pos > *EXPECTED_IN_SIZE.lock().unwrap() {
        return "--- %".to_string();
    }

    // 在未完成时，永远不显示 100.0 %
    let percentage = (in_pos as f64 / *EXPECTED_IN_SIZE.lock().unwrap() as f64) * 99.9;

    // 格式化百分比字符串，保留一位小数
    format!("{:.1} %", percentage)
}

fn progress_sizes(compressed_pos: u64, uncompressed_pos: u64, is_final: bool) -> String {
    use std::fmt::Write;

    let unit_min = if is_final { "B" } else { "MiB" };

    // 假设 uint64_to_nicestr 已经被实现为返回格式化字符串的函数
    fn uint64_to_nicestr(
        val: u64,
        _unit_min: &str,
        _unit_max: &str,
        _flag: bool,
        _precision: u8,
    ) -> String {
        // 这里只是占位符，你需要根据实际逻辑实现
        format!("{:.1} {}", val as f64 / 1024.0, "KiB")
    }

    let compressed = uint64_to_nicestr(compressed_pos, unit_min, "TiB", false, 0);
    let uncompressed = uint64_to_nicestr(uncompressed_pos, unit_min, "TiB", false, 1);

    let mut buf = String::with_capacity(128);
    write!(&mut buf, "{}/{}", compressed, uncompressed).unwrap();

    let ratio = if uncompressed_pos > 0 {
        compressed_pos as f64 / uncompressed_pos as f64
    } else {
        16.0
    };

    if ratio > 9.999 {
        write!(&mut buf, " > {:.3}", 9.999).unwrap();
    } else {
        write!(&mut buf, " = {:.3}", ratio).unwrap();
    }

    buf
}

/// 计算并返回速度字符串，单位为 KiB/s、MiB/s 或 GiB/s
fn progress_speed(uncompressed_pos: u64, elapsed: u64) -> String {
    // 如果时间小于 3 秒，不显示速度
    if elapsed < 3000 {
        return String::new();
    }

    // 单位数组：KiB/s、MiB/s、GiB/s
    let units = ['K', 'M', 'G'];
    let mut unit_index = 0;

    // 计算速度，单位为 KiB/s
    let mut speed = uncompressed_pos as f64 / (elapsed as f64 * (1024.0 / 1000.0));

    // 调整速度单位
    while speed > 999.0 {
        speed /= 1024.0;
        unit_index += 1;
        if unit_index == units.len() {
            return String::new(); // 速度过快，不显示
        }
    }

    // 根据速度大小决定是否显示小数位
    let precision = if speed > 9.9 { 0 } else { 1 };
    format!("{:.precision$} {}iB/s", speed, units[unit_index])
}

/// 生成表示经过时间的字符串，格式为 M:SS 或 H:MM:SS
fn progress_time(mseconds: u64) -> String {
    // 9999 小时 = 416 天
    let mut buf = String::with_capacity(10);

    // 32 位变量足够表示经过的时间（136 年）
    let seconds = (mseconds / 1000) as u32;

    // 如果时间为 0 或过大，不显示
    if seconds == 0 || seconds > ((9999 * 60) + 59) * 60 + 59 {
        return String::new();
    }

    let minutes = seconds / 60;
    let seconds = seconds % 60;

    if minutes >= 60 {
        let hours = minutes / 60;
        let minutes = minutes % 60;
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

/// Return a string containing estimated remaining time when
/// reasonably possible.
fn progress_remaining(in_pos: u64, elapsed: u64) -> &'static str {
    // Don't show the estimated remaining time when it wouldn't
    // make sense:
    //  - Input size is unknown.
    //  - Input has grown bigger since we started (de)compressing.
    //  - We haven't processed much data yet, so estimate would be
    //    too inaccurate.
    //  - Only a few seconds has passed since we started (de)compressing,
    //    so estimate would be too inaccurate.
    if *EXPECTED_IN_SIZE.lock().unwrap() == 0
        || in_pos > *EXPECTED_IN_SIZE.lock().unwrap()
        || in_pos < (1 << 19)
        || elapsed < 8000
    {
        return "";
    }

    // Calculate the estimate. Don't give an estimate of zero seconds,
    // since it is possible that all the input has been already passed
    // to the library, but there is still quite a bit of output pending.
    let remaining = ((*EXPECTED_IN_SIZE.lock().unwrap() - in_pos) as f64
        * (elapsed as f64 / 1000.0)
        / in_pos as f64) as u32;
    let remaining = if remaining < 1 { 1 } else { remaining };

    // Select appropriate precision for the estimated remaining time.
    if remaining <= 10 {
        // A maximum of 10 seconds remaining.
        // Show the number of seconds as is.
        Box::leak(format!("{} s", remaining).into_boxed_str())
    } else if remaining <= 50 {
        // A maximum of 50 seconds remaining.
        // Round up to the next multiple of five seconds.
        let remaining = (remaining + 4) / 5 * 5;
        Box::leak(format!("{} s", remaining).into_boxed_str())
    } else if remaining <= 590 {
        // A maximum of 9 minutes and 50 seconds remaining.
        // Round up to the next multiple of ten seconds.
        let remaining = (remaining + 9) / 10 * 10;
        Box::leak(format!("{} min {} s", remaining / 60, remaining % 60).into_boxed_str())
    } else if remaining <= 59 * 60 {
        // A maximum of 59 minutes remaining.
        // Round up to the next multiple of a minute.
        let remaining = (remaining + 59) / 60;
        Box::leak(format!("{} min", remaining).into_boxed_str())
    } else if remaining <= 9 * 3600 + 50 * 60 {
        // A maximum of 9 hours and 50 minutes left.
        // Round up to the next multiple of ten minutes.
        let remaining = (remaining + 599) / 600 * 10;
        Box::leak(format!("{} h {} min", remaining / 60, remaining % 60).into_boxed_str())
    } else if remaining <= 23 * 3600 {
        // A maximum of 23 hours remaining.
        // Round up to the next multiple of an hour.
        let remaining = (remaining + 3599) / 3600;
        Box::leak(format!("{} h", remaining).into_boxed_str())
    } else if remaining <= 9 * 24 * 3600 + 23 * 3600 {
        // A maximum of 9 days and 23 hours remaining.
        // Round up to the next multiple of an hour.
        let remaining = (remaining + 3599) / 3600;
        Box::leak(format!("{} d {} h", remaining / 24, remaining % 24).into_boxed_str())
    } else if remaining <= 999 * 24 * 3600 {
        // A maximum of 999 days remaining. ;-)
        // Round up to the next multiple of a day.
        let remaining = (remaining + 24 * 3600 - 1) / (24 * 3600);
        Box::leak(format!("{} d", remaining).into_boxed_str())
    } else {
        // The estimated remaining time is too big. Don't show it.
        ""
    }
}

fn progress_pos(in_pos: &mut u64, compressed_pos: &mut u64, uncompressed_pos: &mut u64) {
    let mut out_pos: u64 = 0;

    // 如果处于直通模式，直接从流中获取输入和输出位置
    if PROGRESS_IS_FROM_PASSTHRU.load(Ordering::SeqCst) {
        unsafe {
            let strm_ptr = PROGRESS_STRM.lock().unwrap().0;
            if !strm_ptr.is_null() {
                *in_pos = (*strm_ptr).total_in.get();
                out_pos = (*strm_ptr).total_out.get();
            }
        }
    } else {
        // 否则，使用 lzma_get_progress 获取进度
        unsafe {
            let strm_ptr = PROGRESS_STRM.lock().unwrap().0;
            if !strm_ptr.is_null() {
                lzma_get_progress(&mut *strm_ptr, in_pos, &mut out_pos);
            }
        }
    }

    // 断言：处理后的输入位置不能超过总输入
    unsafe {
        let strm_ptr = PROGRESS_STRM.lock().unwrap().0;
        if !strm_ptr.is_null() {
            assert!(*in_pos <= (*strm_ptr).total_in.get());
        }
    }

    // 断言：输出位置不能小于总输出
    unsafe {
        let strm_ptr = PROGRESS_STRM.lock().unwrap().0;
        if !strm_ptr.is_null() {
            assert!(out_pos >= (*strm_ptr).total_out.get());
        }
    }

    // 根据操作模式设置压缩位置和解压缩位置
    let opt_mode = OPT_MODE.lock().unwrap().clone();
    if opt_mode == OperationMode::Compress {
        *compressed_pos = out_pos;
        *uncompressed_pos = *in_pos;
    } else {
        *compressed_pos = *in_pos;
        *uncompressed_pos = out_pos;
    }
}

/// 更新进度信息
pub fn message_progress_update() {
    // 如果不需要更新进度信息，直接返回
    if !PROGRESS_NEEDS_UPDATING.load(Ordering::SeqCst) {
        return;
    }

    // 计算处理当前文件所花费的时间
    let elapsed = mytime_get_elapsed();

    // 获取当前流中的位置
    let mut in_pos: u64 = 0;
    let mut compressed_pos: u64 = 0;
    let mut uncompressed_pos: u64 = 0;
    progress_pos(&mut in_pos, &mut compressed_pos, &mut uncompressed_pos);

    // 阻塞信号，确保 `writeln!` 不会被中断
    signals_block();

    // 如果当前文件名尚未打印，则打印文件名
    if !CURRENT_FILENAME_PRINTED.load(Ordering::SeqCst) {
        print_filename();
    }

    // 准备进度信息的各个字段
    let cols = [
        progress_percentage(in_pos),
        progress_sizes(compressed_pos, uncompressed_pos, false),
        progress_speed(uncompressed_pos, elapsed),
        progress_time(elapsed),
        progress_remaining(in_pos, elapsed).to_owned(),
    ];

    // 使用 `writeln!` 打印进度信息
    let mut stderr = io::stderr();
    writeln!(
        stderr,
        "\r {:>6} {:>35}   {:>9} {:>10}   {:>10}\r",
        cols[0], cols[1], cols[2], cols[3], cols[4]
    )
    .unwrap();

    // 重置进度更新标志
    PROGRESS_NEEDS_UPDATING.store(false, Ordering::SeqCst);

    // 如果详细程度为 Verbose 且进度指示器是自动的，则设置进度指示器为活跃状态并启动定时器
    if message_verbosity_get() >= MessageVerbosity::Verbose
        && PROGRESS_AUTOMATIC.load(Ordering::SeqCst)
    {
        PROGRESS_ACTIVE.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        {
            sys_unistd::alarm(1); // 1 秒后再次触发进度更新
        }
    } else {
        // 否则，打印换行符以分隔进度信息
        writeln!(stderr).unwrap();
    }

    // 解除信号阻塞
    signals_unblock();
}

/// 刷新并显示最终进度信息
pub fn progress_flush(finished: bool) {
    // 如果进度未开始或详细程度不足，直接返回
    if !PROGRESS_STARTED.load(Ordering::SeqCst)
        || message_verbosity_get() < MessageVerbosity::Verbose
    {
        return;
    }

    // 获取当前处理位置信息
    let mut in_pos: u64 = 0;
    let mut compressed_pos: u64 = 0;
    let mut uncompressed_pos: u64 = 0;
    progress_pos(&mut in_pos, &mut compressed_pos, &mut uncompressed_pos);

    // 如果未完成且进度不活跃，并且没有处理数据，则返回
    if !finished
        && !PROGRESS_ACTIVE.load(Ordering::SeqCst)
        && (compressed_pos == 0 || uncompressed_pos == 0)
    {
        return;
    }

    // 设置进度状态为非活跃
    PROGRESS_ACTIVE.store(false, Ordering::SeqCst);

    // 获取已用时间
    let elapsed = mytime_get_elapsed();

    // 阻塞信号以确保输出不会被中断
    signals_block();

    // 自动模式下的进度显示
    if PROGRESS_AUTOMATIC.load(Ordering::SeqCst) {
        let cols = [
            if finished {
                "100 %"
            } else {
                &progress_percentage(in_pos)
            },
            &progress_sizes(compressed_pos, uncompressed_pos, true),
            &progress_speed(uncompressed_pos, elapsed),
            &progress_time(elapsed),
            if finished {
                ""
            } else {
                progress_remaining(in_pos, elapsed)
            },
        ];

        // 格式化输出进度信息
        let mut stderr = io::stderr();
        writeln!(
            stderr,
            "\r {:>6} {:>35}   {:>9} {:>10}   {:>10}",
            cols[0], cols[1], cols[2], cols[3], cols[4]
        )
        .unwrap();
    } else {
        // 手动模式下的进度显示

        // 总是打印文件名
        let filename_guard = FILENAME.lock().unwrap();
        let filename = filename_guard.as_ref().unwrap();
        write!(io::stderr(), "{}: ", filename).unwrap();

        // 未完成时打印百分比（如果已知）
        if !finished {
            let percentage = progress_percentage(in_pos);
            if !percentage.starts_with("---") {
                write!(io::stderr(), "{}, ", percentage).unwrap();
            }
        }

        // 总是打印大小信息
        write!(
            io::stderr(),
            "{}",
            progress_sizes(compressed_pos, uncompressed_pos, true)
        )
        .unwrap();

        // 速度信息（如果有）
        let speed = progress_speed(uncompressed_pos, elapsed);
        if !speed.is_empty() {
            write!(io::stderr(), ", {}", speed).unwrap();
        }

        // 耗时信息（如果有）
        let elapsed_str = progress_time(elapsed);
        if !elapsed_str.is_empty() {
            write!(io::stderr(), ", {}", elapsed_str).unwrap();
        }

        // 换行结束
        writeln!(io::stderr()).unwrap();
    }

    // 解除信号阻塞
    signals_unblock();
}

/// 结束进度显示并处理结果
pub fn message_progress_end(success: bool) {
    assert!(PROGRESS_STARTED.load(Ordering::SeqCst));
    progress_flush(success);
    PROGRESS_STARTED.store(false, Ordering::SeqCst);
}

// lazy_static! {
//     static ref PROGNAME: String = std::env::args()
//         .next()
//         .unwrap_or_else(|| "utxz".to_string())
//         .split('/')
//         .last()
//         .unwrap_or("utxz")
//         .to_string();
// }
/// 内部消息打印函数 (可变参数版本)
pub fn vmessage(verbosity: MessageVerbosity, fmt: &str, args: std::fmt::Arguments) {
    if verbosity <= message_verbosity_get() {
        signals_block();

        // 先刷新任何未完成的进度显示
        progress_flush(false);

        // 打印程序名前缀 (国际化处理)
        // 注意：法语等语言需要在冒号前加空格
        write!(io::stderr(), "{}: ", PROGNAME.lock().unwrap());

        // 打印实际消息内容
        let rendered_args = args.to_string();
        if rendered_args.is_empty() {
            write!(io::stderr(), "{}", fmt).unwrap();
        } else {
            write!(io::stderr(), "{}", rendered_args).unwrap();
        }
        writeln!(io::stderr()).unwrap();

        signals_unblock();
    }
}

/// 打印普通消息
pub fn message(verbosity: MessageVerbosity, fmt: &str, args: std::fmt::Arguments) {
    vmessage(verbosity, fmt, args);
}

/// 打印警告消息并设置退出状态
// pub fn message_warning(fmt: &str, args: std::fmt::Arguments) {
//     vmessage(MessageVerbosity::Warning, fmt, args);
//     set_exit_status(ExitStatusType::EWarning);
// }

pub fn message_warning(fmt: &str, args: &[&dyn std::fmt::Display]) {
    // 将数组中的值转换为格式化参数
    let formatted_args = args
        .iter()
        .map(|arg| format!("{}", arg))
        .collect::<Vec<_>>()
        .join(", ");

    // 调用 vmessage 函数
    vmessage(
        MessageVerbosity::Warning,
        fmt,
        format_args!("{}", formatted_args),
    );
    set_exit_status(ExitStatusType::EWarning);
}

/// 打印错误消息并设置退出状态
pub fn message_error(fmt: &str, args: std::fmt::Arguments) {
    vmessage(MessageVerbosity::Error, fmt, args);
    set_exit_status(ExitStatusType::EError);
}

/// 打印致命错误消息并退出程序
pub fn message_fatal(fmt: &str, args: std::fmt::Arguments) {
    vmessage(MessageVerbosity::Error, fmt, args);
    tuklib_exit(crate::E_ERROR, crate::E_ERROR, 0);
}

/// 处理内部错误（Bug）并退出程序
///
/// 该函数用于处理程序内部的错误（Bug），打印错误消息后立即退出程序。
pub fn message_bug() {
    message_fatal("内部错误（Bug）", format_args!(""));
}

/// 处理信号处理程序无法建立的错误并退出程序
///
/// 该函数用于处理信号处理程序无法建立的错误，打印错误消息后立即退出程序。
pub fn message_signal_handler() {
    message_fatal("无法建立信号处理程序", format_args!(""));
}

/// 根据 `lzma_ret` 错误码返回对应的错误消息
///
/// 该函数用于将 `lzma_ret` 错误码转换为可读的错误消息。
///
/// # 参数
/// - `code`: `lzma_ret` 错误码
///
/// # 返回值
/// 返回与错误码对应的错误消息字符串
pub fn message_strm(code: LzmaRet) -> &'static str {
    match code {
        LzmaRet::NoCheck => "未进行完整性检查；未验证文件完整性",
        LzmaRet::UnsupportedCheck => "不支持的完整性检查类型；未验证文件完整性",
        LzmaRet::MemError => "内存不足",
        LzmaRet::MemlimitError => "已达到内存使用限制",
        LzmaRet::FormatError => "文件格式无法识别",
        LzmaRet::OptionsError => "不支持的选项",
        LzmaRet::DataError => "压缩数据已损坏",
        LzmaRet::BufError => "输入意外结束",
        _ => "内部错误（Bug）", // 其他未处理的错误码
    }
}

/// 显示所需内存信息
///
/// # 参数
/// - `verbosity`: 消息详细程度
/// - `memusage`: 所需内存大小(字节)
pub fn message_mem_needed(verbosity: MessageVerbosity, memusage: u64) {
    // 如果请求的详细程度低于当前设置，直接返回
    if verbosity > message_verbosity_get() {
        return;
    }

    // 将内存使用量舍入到MiB
    let memusage = round_up_to_mib(memusage);

    // 获取硬件内存限制
    let opt_mode = OPT_MODE.lock().unwrap();
    let memlimit = hardware_memlimit_get(opt_mode.clone());

    // 如果内存限制器被禁用
    if memlimit == u64::MAX {
        message(
            verbosity,
            "{} MiB 内存是必需的。内存限制器已禁用。",
            format_args!("{}", uint64_to_str(memusage, 0)),
        );
        return;
    }

    // 格式化内存限制字符串
    let memlimitstr = if memlimit < (1 << 20) {
        format!("{} B", uint64_to_str(memlimit, 1))
    } else {
        format!("{} MiB", uint64_to_str(round_up_to_mib(memlimit), 1))
    };

    message(
        verbosity,
        "{} MiB 内存是必需的。限制是 {}。",
        format_args!("{} {} ", uint64_to_str(memusage, 0), memlimitstr),
    );
}

/// 显示过滤器链信息
///
/// # 参数
/// - `verbosity`: 消息详细程度
/// - `filters`: 过滤器链
pub fn message_filters_show(verbosity: MessageVerbosity, filters: &[LzmaFilter]) {
    if verbosity > message_verbosity_get() {
        return;
    }

    // 将过滤器转换为字符串表示
    // let buf = match lzma_str_from_filters(
    //     filters,
    //     LzmaFilter::ENCODER | lzma::StrFlags::GETOPT_LONG,
    //     None
    // ) {
    //     Ok(s) => s,
    //     Err(e) => {
    //         message_fatal("{}", format_args!("{}", message_strm(e)));
    //         return;
    //     }
    // };

    // // 打印过滤器链信息
    // message(
    //     verbosity,
    //     "{}: 过滤器链: {}",
    //     format_args!("{}", *PROGNAME),
    //     buf
    // );
}

/// 显示尝试获取帮助的建议信息
pub fn message_try_help() {
    // 使用警告级别而不是错误级别，防止在使用--quiet时显示
    message(
        MessageVerbosity::Warning,
        "尝试 `{} --help` 获取更多信息。",
        format_args!("{}", PROGNAME.lock().unwrap()),
    );
}

/// 打印版本信息
pub fn message_version() {
    // 如果启用了机器人模式（`opt_robot`），则输出机器可读的版本信息
    if *OPT_STDOUT.lock().unwrap() {
        println!(
            "XZ_VERSION={}\nLIBLZMA_VERSION={}",
            LZMA_VERSION,
            lzma_version_number()
        );
    } else {
        // 否则，输出人类可读的版本信息
        println!(
            "utxz ({}) {} {} {} {} {} {} {}",
            "UTXZ Utils", "0", ".", "0", ".", "1", "", ""
        );
        println!("liblzma {}", lzma_version_string());
    }

    // 根据详细程度决定是否退出程序
    tuklib_exit(
        ExitStatusType::ESuccess as i32,
        ExitStatusType::EError as i32,
        (message_verbosity_get() != MessageVerbosity::Silent) as i32,
    );
}

/// 显示帮助信息
///
/// # 参数
/// - `long_help`: 是否显示详细帮助信息
pub fn message_help(long_help: bool) {
    // 打印基本用法信息
    println!("用法: {} [选项]... [文件]...", *PROGNAME.lock().unwrap());
    println!("以 .xz 格式压缩或解压文件\n");

    // 打印基本操作选项
    println!(
        "{:>4}-z, --compress     强制压缩\n\
         {:>4}-d, --decompress   强制解压\n\
         {:>4}-f, --force        强制覆盖输出文件和(解)压缩链接\n\
         {:>4}-l, --list         列出关于 .xz 文件的信息\n\
         {:>4}-T, --threads N   使用 N 个线程进行压缩（缺省：1）\n\
         {:>4}                    -T0 使用系统所有 CPU 线程",
        "", "", "", "", "", ""
    );

    println!("{:>4}-h, --help        显示此简短帮助并退出", "");

    println!(
        "{:>4}-V, --version     显示版本号并退出\n\n\
          {:>4}没有文件或文件为-时，从标准输入读取",
        "", ""
    );

    // // 打印错误报告信息
    // println!("报告错误至 <{}> (英文或芬兰文)", "utxz-bugs@example.com");
    // println!(
    //     "{} 主页: <{}>",
    //     "UTXZ Utils", "https://github.com/utxz/utxz"
    // );

    // 根据详细程度决定退出状态
    tuklib_exit(
        ExitStatusType::ESuccess as i32,
        ExitStatusType::EError as i32,
        (message_verbosity_get() != MessageVerbosity::Silent) as i32,
    );
}
