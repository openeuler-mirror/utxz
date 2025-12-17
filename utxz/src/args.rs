/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
// use std::process::Command;
use crate::coder::{
    coder_set_compression_settings, get_opt_format, set_opt_format, set_opt_mode, FormatType,
    OperationMode, CHECK, OPT_BLOCK_LIST, OPT_FORMAT, OPT_MODE,
};
use crate::hardware::{hardware_memlimit_set, hardware_threads_set};
use crate::message::message_help;
use crate::suffix::suffix_is_set;
use crate::util::str_to_uint64;
use clap::{Arg, ArgAction, ArgMatches, Command};
use lazy_static::lazy_static;
use std::error::Error;
use std::str;
use std::sync::Mutex;

/// 定义文件分隔符常量
pub const DEFAULT_FILES_DELIM: char = '\n'; // 假设默认分隔符为换行符
pub static STDIN_FILENAME: &str = "(stdin)";

/// 命令行参数信息结构体
#[derive(Debug)]
pub struct ArgsInfo {
    /// 命令行传入的文件名列表
    pub arg_names: Vec<String>,

    pub arg_count: u32,

    /// 从文件中读取文件名的源文件路径(当使用--files或--files0时设置)
    pub files_name: Option<String>,

    /// 打开用于读取文件名的文件句柄(与files_name同时存在)
    pub files_file: Option<File>,

    /// 从files_file读取文件名时使用的分隔符
    pub files_delim: char,
}

impl ArgsInfo {
    /// 创建一个新的空ArgsInfo实例
    pub fn new() -> Self {
        ArgsInfo {
            arg_names: Vec::new(),
            arg_count: 0,
            files_name: None,
            files_file: None,
            files_delim: '\n', // 默认使用换行符分隔
        }
    }

    /// 获取参数数量
    pub fn arg_count(&self) -> usize {
        self.arg_names.len()
    }

    /// 添加一个文件名
    pub fn add_arg_name(&mut self, name: String) {
        self.arg_names.push(name);
    }

    /// 设置文件来源
    pub fn set_files_source(&mut self, name: String, delim: char) -> io::Result<()> {
        self.files_name = Some(name.clone());
        self.files_delim = delim;
        self.files_file = Some(File::open(name)?);
        Ok(())
    }
}

lazy_static! {
    pub static ref OPT_STDOUT: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_FORCE: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_KEEP_ORIGINAL: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_ROBOT: Mutex<bool> = Mutex::new(false);
    pub static ref OPT_IGNORE_CHECK: Mutex<bool> = Mutex::new(false);
}

/// 解析内存限制参数
///
/// # 参数
/// - `name`: 参数名称（用于错误提示）
/// - `name_percentage`: 百分比参数名称
/// - `str`: 输入的字符串值（如 "50%" 或 "1024M"）
/// - `set_compress`: 是否设置压缩内存限制
/// - `set_decompress`: 是否设置解压内存限制
/// - `set_mtdec`: 是否设置多线程解压内存限制
fn parse_memlimit(
    name: &str,
    name_percentage: &str,
    str: &str,
    set_compress: bool,
    set_decompress: bool,
    set_mtdec: bool,
) {
    let mut is_percentage = false;
    let value: u64;

    if let Some(percent_str) = str.strip_suffix('%') {
        // 处理百分比值 (1-100)
        is_percentage = true;
        value = str_to_uint64(name_percentage, percent_str, 1, 100);
    } else {
        // 处理绝对值 (0-U64::MAX)
        value = str_to_uint64(name, str, 0, u64::MAX);
    }

    hardware_memlimit_set(
        value,
        set_compress,
        set_decompress,
        set_mtdec,
        is_percentage,
    );
}

/// 解析块列表参数
///
/// # 参数
/// - `str_const`: 逗号分隔的块大小列表（如 "1M,2M,0"）
fn parse_block_list(str_const: &str) {
    // 验证输入有效性
    if str_const.is_empty() || str_const.starts_with(',') {
        panic!("无效的块列表参数: {}", str_const);
    }

    // 分割字符串并计算块数
    let blocks: Vec<&str> = str_const.split(',').collect();
    let count = blocks.len();

    // 防止溢出
    if count > usize::MAX / std::mem::size_of::<u64>() - 1 {
        panic!("块列表参数过多: {}", str_const);
    }

    // 分配内存并解析每个块
    let mut opt_block_list: Vec<u64> = Vec::with_capacity(count + 1);
    let mut prev_value = 0;

    for (i, block_str) in blocks.iter().enumerate() {
        let value = if block_str.is_empty() {
            // 空值使用前一个值
            prev_value
        } else {
            // 解析数值
            let v = str_to_uint64("block-list", block_str, 0, u64::MAX);

            // 0 只能作为最后一个元素
            if v == 0 && i != count - 1 {
                panic!("0 只能用作块列表的最后一个元素");
            }

            if v == 0 {
                u64::MAX
            } else {
                v
            }
        };

        opt_block_list.push(value);
        prev_value = value;
    }

    // 终止标记
    opt_block_list.push(0);

    // 更新全局块列表
    *OPT_BLOCK_LIST.lock().unwrap() = Some(opt_block_list);
}

/// 解析命令行参数
pub fn parse_real(args: &mut ArgsInfo) -> ArgMatches {
    let commands = Command::new("utxz")
        .version("1.0")
        .author("Your Name")
        .about("A Rust implementation of utxz")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .trailing_var_arg(true) // 允许接收额外的位置参数
        .arg(
            Arg::new("compress")
                .short('z')
                .long("compress")
                .action(ArgAction::Append) // 每次出现都追加
                .num_args(0..) // 接受0或多个参数
                .value_name("FILES..."), // 参数名
                                         // .help("压缩指定的文件"),
        )
        .arg(
            Arg::new("decompress")
                .short('d')
                .long("decompress")
                .action(ArgAction::Append)
                .num_args(0..),
        )
        .arg(
            Arg::new("test")
                .short('t')
                .long("test")
                .action(ArgAction::SetTrue), // .help("测试文件完整性"),
        )
        .arg(
            Arg::new("list")
                .short('l')
                .long("list")
                .action(ArgAction::Append) // .help("列出文件信息"),
                .num_args(1..),
        )
        // 操作修饰符
        .arg(
            Arg::new("keep")
                .short('k')
                .long("keep")
                .action(ArgAction::SetTrue), // .help("保留原始文件"),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue), // .help("强制覆盖文件"),
        )
        .arg(
            Arg::new("stdout")
                .short('c')
                .long("stdout")
                .action(ArgAction::SetTrue), // .help("输出到标准输出"),
        )
        .arg(
            Arg::new("single-stream")
                .long("single-stream")
                .action(ArgAction::SetTrue), // .help("单流模式"),
        )
        .arg(
            Arg::new("no-sparse")
                .long("no-sparse")
                .action(ArgAction::SetTrue), // .help("禁用稀疏文件处理"),
        )
        .arg(
            Arg::new("suffix")
                .short('S')
                .long("suffix")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置后缀名"),
        )
        .arg(
            Arg::new("files")
                .long("files")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("从文件读取输入"),
        )
        // 基本压缩设置
        .arg(
            Arg::new("format")
                .short('F')
                .long("format")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置文件格式"),
        )
        .arg(
            Arg::new("check")
                .short('C')
                .long("check")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置校验类型"),
        )
        .arg(
            Arg::new("ignore-check")
                .long("ignore-check")
                .action(ArgAction::SetTrue), // .help("忽略校验"),
        )
        .arg(
            Arg::new("block-size")
                .long("block-size")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置块大小"),
        )
        .arg(
            Arg::new("block-list")
                .long("block-list")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置块列表"),
        )
        .arg(
            Arg::new("memlimit-compress")
                .long("memlimit-compress")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置压缩内存限制"),
        )
        .arg(
            Arg::new("memlimit-decompress")
                .long("memlimit-decompress")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置解压内存限制"),
        )
        .arg(
            Arg::new("memlimit-mt-decompress")
                .long("memlimit-mt-decompress")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置多线程解压内存限制"),
        )
        .arg(
            Arg::new("memlimit")
                .short('M')
                .long("memlimit")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置内存限制"),
        )
        .arg(Arg::new("no-adjust").long("no-adjust").help("禁用自动调整"))
        .arg(
            Arg::new("threads")
                .short('T')
                .long("threads")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置线程数"),
        )
        .arg(
            Arg::new("flush-timeout")
                .long("flush-timeout")
                .num_args(1)
                .action(ArgAction::SetTrue), // .help("设置刷新超时"),
        )
        // 其他选项
        .arg(Arg::new("quiet").short('q').long("quiet").help("静默模式"))
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::SetTrue), // .help("详细模式"),
        )
        .arg(
            Arg::new("no-warn")
                .short('Q')
                .long("no-warn")
                .action(ArgAction::SetTrue), // .help("禁用警告"),
        )
        .arg(Arg::new("robot").long("robot").help("机器人模式"))
        .arg(
            Arg::new("info-memory")
                .long("info-memory")
                .action(ArgAction::SetTrue), // .help("显示内存信息"),
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue), // .help("显示帮助信息"),
        )
        .arg(
            Arg::new("long-help")
                .short('H')
                .long("long-help")
                .action(ArgAction::SetTrue), // .help("显示详细帮助信息"),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::SetTrue), // .help("显示版本信息"),
        );

    let matches: clap::ArgMatches = match commands.try_get_matches() {
        Ok(matches) => matches,
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(err.exit_code());
            ArgMatches::default() // 这行代码实际上永远不会执行
        }
    };

    // 处理参数
    if matches.contains_id("compress") {
        // println!("compress mode");
        set_opt_mode(OperationMode::Compress);

        // 获取 compress 后面的所有参数
        if let Some(files) = matches.get_many::<String>("compress") {
            args.arg_names = files.map(|s| s.to_string()).collect();
            args.arg_count = args.arg_names.len() as u32;
        } else {
            args.arg_names = vec!["-".to_string()];
            args.arg_count = 1;
        }
    }
    if matches.contains_id("decompress") {
        set_opt_mode(OperationMode::Decompress);
        if let Some(files) = matches.get_many::<String>("decompress") {
            args.arg_names = files.map(|s| s.to_string()).collect();
            args.arg_count = args.arg_names.len() as u32;
        } else {
            args.arg_names = vec!["-".to_string()];
            args.arg_count = 1;
        }
    }
    if matches.get_flag("test") {
        set_opt_mode(OperationMode::Test);
    }
    if matches.contains_id("list") {
        set_opt_mode(OperationMode::List);
        // 获取 compress 后面的所有参数
        if let Some(files) = matches.get_many::<String>("list") {
            args.arg_names = files.map(|s| s.to_string()).collect();
            args.arg_count = args.arg_names.len() as u32;
        } else {
            args.arg_names = vec!["-".to_string()];
            args.arg_count = 1;
        }
    }

    if matches.get_flag("force") {
        *OPT_FORCE.lock().unwrap() = true;
    }

    if matches.get_flag("long-help") {
        message_help(true);
    }
    if matches.get_flag("help") {
        message_help(false);
    }
    if matches.get_flag("version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
    }

    // 处理内存限制
    // if let Some(memlimit) = matches
    //     .get_one::<String>("memlimit-compress")
    //     .map(String::as_str)
    // {
    //     parse_memlimit(
    //         "memlimit-compress",
    //         "memlimit-compress%",
    //         memlimit,
    //         true,
    //         false,
    //         false,
    //     );
    // }
    // if let Some(memlimit) = matches
    //     .get_one::<String>("memlimit-decompress")
    //     .map(String::as_str)
    // {
    //     parse_memlimit(
    //         "memlimit-decompress",
    //         "memlimit-decompress%",
    //         memlimit,
    //         false,
    //         true,
    //         false,
    //     );
    // }
    // if let Some(memlimit) = matches
    //     .get_one::<String>("memlimit-mt-decompress")
    //     .map(String::as_str)
    // {
    //     parse_memlimit(
    //         "memlimit-mt-decompress",
    //         "memlimit-mt-decompress%",
    //         memlimit,
    //         false,
    //         false,
    //         true,
    //     );
    // }
    // if let Some(memlimit) = matches.get_one::<String>("memlimit").map(String::as_str) {
    //     parse_memlimit("memlimit", "memlimit%", memlimit, true, true, true);
    // }

    // // 处理线程数
    // if let Some(threads) = matches.get_one::<String>("threads").map(String::as_str) {
    //     let t = str_to_uint64("threads", threads, 0, 16384);
    //     hardware_threads_set(t as u32);
    // }

    // // 处理块列表
    // if let Some(block_list) = matches.get_one::<String>("block-list").map(String::as_str) {
    //     parse_block_list(block_list);
    // }

    matches
}

/// 解析环境变量中的参数
///
/// # 参数
/// - `args`: 参数信息结构体
/// - `argv0`: 程序名称
/// - `varname`: 环境变量名
pub fn parse_environment(args: &mut ArgsInfo, argv0: &str, varname: &str) {
    // 获取环境变量值
    let env = match std::env::var(varname) {
        Ok(val) => val,
        Err(_) => return, // 环境变量不存在则直接返回
    };

    // 计算参数数量(从1开始，包含程序名)
    let mut argc = 1;
    let mut prev_was_space = true;

    // 第一遍遍历：计算参数数量
    for c in env.chars() {
        if c.is_whitespace() {
            prev_was_space = true;
        } else if prev_was_space {
            prev_was_space = false;
            argc += 1;

            // 防止参数数量溢出
            if argc == std::cmp::min(i32::MAX, (usize::MAX / std::mem::size_of::<&str>()) as i32) {
                panic!("环境变量 {} 包含过多参数", varname);
            }
        }
    }

    // 准备参数数组(额外空间用于终止NULL)
    let mut argv: Vec<&str> = Vec::with_capacity((argc + 1).try_into().unwrap());
    argv.push(argv0);

    // 第二遍遍历：分割参数
    let env_str = env.as_str();
    let mut start = 0;
    prev_was_space = true;

    for (i, c) in env.chars().enumerate() {
        if c.is_whitespace() {
            prev_was_space = true;
            if !prev_was_space {
                argv.push(&env_str[start..i]);
            }
            start = i + c.len_utf8();
        } else if prev_was_space {
            prev_was_space = false;
            start = i;
        }
    }

    // 添加最后一个参数
    if !prev_was_space {
        argv.push(&env_str[start..]);
    }

    // 终止标记
    argv.push("");

    // 解析实际参数(忽略非选项参数)
    parse_real(args);
}

/// 解析命令行参数
pub fn args_parse(args: &mut ArgsInfo, argc: i32, argv: Vec<&str>) {
    // 初始化需要的部分
    args.files_name = None;
    args.files_file = None;
    args.files_delim = '\0';

    // 检查程序名称以确定操作模式
    if let Some(name) = argv[0].rsplit('/').next() {
        if name.contains("xzcat") {
            *OPT_MODE.lock().unwrap() = OperationMode::Decompress;
            *OPT_STDOUT.lock().unwrap() = true;
        } else if name.contains("unxz") {
            *OPT_MODE.lock().unwrap() = OperationMode::Decompress;
        } else if name.contains("lzcat") {
            *OPT_FORMAT.lock().unwrap() = FormatType::Lzma;
            *OPT_MODE.lock().unwrap() = OperationMode::Decompress;
            *OPT_STDOUT.lock().unwrap() = true;
        } else if name.contains("unlzma") {
            *OPT_FORMAT.lock().unwrap() = FormatType::Lzma;
            *OPT_MODE.lock().unwrap() = OperationMode::Decompress;
        } else if name.contains("lzma") {
            *OPT_FORMAT.lock().unwrap() = FormatType::Lzma;
        }
    }

    // 首先解析环境变量中的参数 一般情况下 这两个函数没有影响
    parse_environment(args, argv[0], "XZ_DEFAULTS");
    parse_environment(args, argv[0], "XZ_OPT");

    // 然后解析命令行参数
    parse_real(args);

    // 如果压缩模式且格式为 Lzip，报错
    if *OPT_MODE.lock().unwrap() == OperationMode::Compress
        && *OPT_FORMAT.lock().unwrap() == FormatType::Lzip
    {
        panic!("不支持压缩 Lzip 文件 (.lz)");
    }

    // 如果输出到标准输出或测试模式，保留原始文件
    if *OPT_STDOUT.lock().unwrap() || *OPT_MODE.lock().unwrap() == OperationMode::Test {
        *OPT_KEEP_ORIGINAL.lock().unwrap() = true;
        *OPT_STDOUT.lock().unwrap() = true;
    }

    // 如果压缩模式且格式为 Auto，默认使用 Xz 格式
    if *OPT_MODE.lock().unwrap() == OperationMode::Compress
        && *OPT_FORMAT.lock().unwrap() == FormatType::Auto
    {
        *OPT_FORMAT.lock().unwrap() = FormatType::Xz;
    }

    if *OPT_MODE.lock().unwrap() == OperationMode::Compress
        || (*OPT_FORMAT.lock().unwrap() == FormatType::Raw
            && *OPT_MODE.lock().unwrap() != OperationMode::List)
    {
        coder_set_compression_settings();
    }

    // 如果使用 Raw 格式且未设置后缀，且不是输出到标准输出，报错
    if *OPT_FORMAT.lock().unwrap() == FormatType::Raw
        && !suffix_is_set()
        && !*OPT_STDOUT.lock().unwrap()
    {
        panic!("使用 --format=raw 时，必须指定 --suffix=.SUF 或输出到标准输出");
    }

    // println!("args_names{:#?}", args);
}
