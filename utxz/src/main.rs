/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(unused)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

mod args;
mod coder;
mod file_io;
mod hardware;
mod list;
mod message;
mod mytime;
mod options;
mod signals;
mod suffix;
mod util;

use crate::args::parse_real;
use args::{args_parse, ArgsInfo, OPT_ROBOT, OPT_STDOUT, STDIN_FILENAME};
use coder::{coder_run, OperationMode, OPT_MODE};
use common::{tuklib_exit, PROGNAME};
use file_io::io_init;
use hardware::hardware_init;
use lazy_static::lazy_static;
use libc::{fclose, strcmp};
use list::{list_file, list_totals};
use message::{
    message_error, message_fatal, message_init, message_set_files, message_try_help,
    message_verbosity_get, MessageVerbosity,
};
use signals::{signals_exit, signals_init, USER_ABORT};
use std::{
    env,
    io::{self, Read},
    sync::{Mutex, Once},
    thread,
    time::Duration,
};
use util::{is_tty_stdin, is_tty_stdout};

const E_SUCCESS: i32 = 0;
const E_ERROR: i32 = 1;
const E_WARNING: i32 = 2;

// 定义 exit_status_type 枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)] // 确保与 C 的内存布局兼容
pub enum ExitStatusType {
    ESuccess = 0,
    EError = 1,
    EWarning = 2,
}

// 使用 lazy_static 来创建全局静态变量
lazy_static! {
    static ref EXIT_STATUS: Mutex<ExitStatusType> = Mutex::new(ExitStatusType::ESuccess);
    static ref NO_WARN: Mutex<bool> = Mutex::new(false);
}

pub fn set_exit_status(new_status: ExitStatusType) {
    // 确保 new_status 为 E_WARNING 或 E_ERROR
    assert!(new_status == ExitStatusType::EWarning || new_status == ExitStatusType::EError);

    // 更新 exit_status（如果不等于 E_ERROR）
    if *EXIT_STATUS.lock().unwrap() != ExitStatusType::EError {
        *EXIT_STATUS.lock().unwrap() = new_status;
    }
}

pub fn set_exit_no_warn() {
    // 设置 no_warn 为 true，表示不设置退出状态为 E_WARNING

    *NO_WARN.lock().unwrap() = true;
}

// 假设 USER_ABORT 是全局 Mutex<bool>
fn read_name(args: &mut ArgsInfo) -> Option<String> {
    // 初始分配 256 字节容量的缓冲区（String 会自动扩容）
    let mut name = String::with_capacity(256);

    // 循环读取，直到用户中断
    while !*USER_ABORT.lock().unwrap() {
        // 定义一个 1 字节缓冲数组
        let mut buf = [0u8; 1];

        // 先解包 Option<File>
        let file = match args.files_file.as_mut() {
            Some(f) => f,
            None => {
                eprintln!("{:#?}: 文件句柄不存在", args.files_name);
                return None;
            }
        };

        // 尝试读取1个字节
        match file.read(&mut buf) {
            Ok(0) => {
                // 遇到文件结尾
                if !name.is_empty() {
                    eprintln!("{:#?}: 读取文件名时遇到意外的输入结束", args.files_name);
                }
                return None;
            }
            Ok(_) => {
                let c = buf[0];
                // 如果读取到分隔符，则认为一个文件名结束
                if c == args.files_delim as u8 {
                    // 忽略空文件名（连续分隔符）
                    if name.is_empty() {
                        continue;
                    }
                    return Some(name);
                }
                // 如果读取到 '\0' 字符，而分隔符不是 '\0'，则报错
                if c == 0 {
                    eprintln!(
                        "{:#?}: 读取文件名时发现空字符；也许你应该用 \"--files0\" 而不是 \"--files\"",
                        args.files_name
                    );
                    return None;
                }
                // 将读取到的字节转换为 char 并追加到缓冲区
                // 注意：本例假定输入为 ASCII 或 UTF-8 编码，单字节对应一个有效字符
                name.push(c as char);
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                // 如果错误为 EINTR（被中断），则等待一段时间后重试
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(e) => {
                eprintln!("{:#?}: 读取文件名时出错: {}", args.files_name, e);
                return None;
            }
        }
    }
    // 用户中断返回 None
    None
}

// static mut PROGNAME: Option<String> = None;

/// 初始化全局程序名
fn tuklib_progname_init() {
    // 获取命令行参数的第一个（程序名）
    if let Some(name) = std::env::args().next() {
        *PROGNAME.lock().unwrap() = name;
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    // for arg in &args {
    //     println!("{}", arg);
    // }
    let mut args_info = ArgsInfo {
        files_file: None,
        files_name: None,
        files_delim: '\n', // Default delimiter as newline
        arg_count: 0,
        arg_names: args.clone(),
    };

    // Set up program name
    tuklib_progname_init();

    // Initialize file I/O (stdin, stdout, stderr)
    io_init();

    message_init();
    hardware_init();
    // Parse command line arguments

    let argv: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    args_parse(&mut args_info, args.len() as i32, argv);

    // // Handle unsupported robot mode
    if *OPT_MODE.lock().unwrap() != OperationMode::List && *OPT_ROBOT.lock().unwrap() {
        // Just an example, replace with actual condition
        message_fatal(
            "Compression and decompression with --robot are not supported yet.",
            format_args!(""),
        );
    }

    // If we know the number of files, update message handling
    if args_info.files_name.is_some() {
        message_set_files(0);
    } else {
        message_set_files(args_info.arg_count);
    }

    // Compression mode check
    if *OPT_MODE.lock().unwrap() == OperationMode::Compress {
        // Just an example, replace with actual condition
        if *OPT_STDOUT.lock().unwrap()
            || (args_info.arg_count == 1 && args_info.arg_names[0] == "-")
        {
            if (is_tty_stdout()) {
                message_try_help();
                tuklib_exit(E_ERROR, E_ERROR, 0);
            }
        }
    }

    // Initialize signal handlers
    if *OPT_MODE.lock().unwrap() != OperationMode::List {
        // Just an example, replace with actual condition
        signals_init();
    }

    // 选择运行函数
    let run_fn: fn(&str) = if *OPT_MODE.lock().unwrap() == OperationMode::List {
        list_file
    } else {
        coder_run
    };
    // Process the files given on the command line
    for i in 0..args_info.arg_count {
        if args_info.arg_names[i as usize] == "-" {
            // Handle stdin/stdout cases
            if *OPT_MODE.lock().unwrap() == OperationMode::Compress {
                // Compression mode check
                if is_tty_stdout() {
                    continue;
                }
            } else if is_tty_stdin() {
                continue;
            }

            // Handle stdin filename
            if args_info.files_name == Some(String::from("stdin")) {
                message_error("Cannot read data from standard input when reading filenames from standard input", format_args!(""));
                continue;
            }

            // Replace the "-" with a special pointer for stdin
            args_info.arg_names[i as usize] = "stdin".to_string();
        }

        // Call the run function (compression or decompression)

        run_fn(&args_info.arg_names[i as usize]);
    }

    // If --files or --files0 was used, process the filenames

    if let Some(files_name) = &args_info.files_name {
        while let Some(name) = read_name(&mut args_info) {
            run_fn(&name);
        }
        if args_info.files_name != Some(STDIN_FILENAME.to_string()) {
            if let Some(file) = args_info.files_file.take() {
                drop(file);
            }
        }
    }

    if (*OPT_MODE.lock().unwrap() == OperationMode::List) {
        assert!(!*USER_ABORT.lock().unwrap());
        list_totals();
    }

    signals_exit();

    let mut es: ExitStatusType = *EXIT_STATUS.lock().unwrap();
    if (es == ExitStatusType::EError && *NO_WARN.lock().unwrap()) {
        es = ExitStatusType::ESuccess;
    }

    // Exit handling
    tuklib_exit(
        es as i32,
        E_ERROR,
        (message_verbosity_get() != MessageVerbosity::Silent) as i32,
    );
}
