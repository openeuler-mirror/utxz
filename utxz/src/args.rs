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
/// 解析命令行参数
pub fn parse_real(args: &mut ArgsInfo) -> ArgMatches {
    let commands = Command::new("utxz")
        .version("1.0")
        .author("Your Name")
        .about("A Rust implementation of utxz")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .trailing_var_arg(true)
        .arg(
            Arg::new("compress")
                .short('z')
                .long("compress")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("decompress")
                .short('d')
                .long("decompress")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("list")
                .short('l')
                .long("list")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("threads")
                .short('T')
                .long("threads")
                .action(ArgAction::Set)
                .value_name("NUM"),
        )
        .arg(
            Arg::new("files")
                .action(ArgAction::Append)
                .num_args(0..)
                .value_name("FILE"),
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
    if matches.get_flag("compress") {
        set_opt_mode(OperationMode::Compress);
    }
    if matches.get_flag("decompress") {
        set_opt_mode(OperationMode::Decompress);
    }

    if matches.get_flag("list") {
        set_opt_mode(OperationMode::List);
    }

    // 从 files 位置参数获取文件列表
    if let Some(files) = matches.get_many::<String>("files") {
        args.arg_names = files.map(|s| s.to_string()).collect();
        args.arg_count = args.arg_names.len() as u32;
    } else {
        args.arg_names = vec!["-".to_string()];
        args.arg_count = 1;
    }

    if matches.get_flag("help") {
        message_help(false);
    }
    if matches.get_flag("version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
    }
    if matches.get_flag("force") {
        *OPT_FORCE.lock().unwrap() = true;
    }

    if let Some(threads_str) = matches.get_one::<String>("threads") {
        let threads = str_to_uint64("threads", threads_str, 0, u32::MAX as u64) as u32;
        hardware_threads_set(threads);
    }

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
