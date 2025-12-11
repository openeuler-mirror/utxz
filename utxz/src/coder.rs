/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

// use once_cell::sync::Lazy;
#![allow(unused_variables)]
#![warn(unused_assignments)]
use lazy_static::lazy_static;
use liblzma::{
    api::{
        LzmaAction, LzmaCheck, LzmaFilter, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream,
        LzmaVli, LZMA_CONCATENATED, LZMA_FILTERS_MAX, LZMA_FILTER_LZMA1, LZMA_FILTER_LZMA2,
        LZMA_IGNORE_CHECK, LZMA_PRESET_DEFAULT, LZMA_PRESET_EXTREME, LZMA_PRESET_LEVEL_MASK,
        LZMA_TELL_UNSUPPORTED_CHECK,
    },
    check::lzma_check_is_supported,
    common::{
        lzma_alone_decoder, lzma_alone_encoder, lzma_code, lzma_lzip_decoder, lzma_memusage,
        lzma_properties_decode, lzma_raw_decoder, lzma_raw_decoder_memusage, lzma_raw_encoder,
        lzma_raw_encoder_memusage, lzma_stream_decoder, lzma_stream_encoder,
    },
    lzma::lzma_lzma_preset,
};
use std::sync::{Arc, Mutex, Weak};

use crate::{
    args::{OPT_FORCE, OPT_IGNORE_CHECK, OPT_STDOUT},
    file_io::{
        io_close, io_fix_src_pos, io_open_dest, io_open_src, io_read, io_write, FilePair, IoBuf,
        IO_BUFFER_SIZE,
    },
    hardware::{hardware_memlimit_get, hardware_threads_is_mt},
    message::{
        message, message_filename, message_mem_needed, message_progress_end,
        message_progress_start, message_progress_update, message_strm, MessageVerbosity,
    },
    mytime::{mytime_set_start_time, OPT_FLUSH_TIMEOUT},
    signals::USER_ABORT,
    util::round_up_to_mib,
};
/// coder_init() 返回值的类型
#[derive(Debug, PartialEq)]
pub enum CoderInitRet {
    Normal,
    PassThru,
    Error,
}

/// 当前的操作模式
#[derive(Debug, PartialEq, Clone)]
#[repr(u32)]
pub enum OperationMode {
    Compress,
    Decompress,
    Test,
    List,
}

/// 当前的格式类型
#[derive(Debug, PartialEq, Clone, Copy)]
#[repr(u32)]
pub enum FormatType {
    Auto,
    Xz,
    Lzma,
    Lzip,
    Raw,
    // 你可以根据实际需要添加更多格式类型
}

lazy_static! {
    static ref STRM: Mutex<LzmaStream<'static>> = Mutex::new(LzmaStream::default());

    /// 当前操作模式，默认为 MODE_COMPRESS
    pub static ref OPT_MODE: Mutex<OperationMode> = Mutex::new(OperationMode::Compress);

    /// 当前文件格式，默认为 FORMAT_AUTO
    pub static ref OPT_FORMAT: Mutex<FormatType> = Mutex::new(FormatType::Auto);

    /// 自动调整标志，默认为 true
    pub static ref OPT_AUTO_ADJUST: Mutex<bool> = Mutex::new(true);

    /// 单流模式标志，默认为 false
    pub static ref OPT_SINGLE_STREAM: Mutex<bool> = Mutex::new(false);

    /// 块大小，用于分块压缩，默认为 0
    pub static ref OPT_BLOCK_SIZE: Mutex<u64> = Mutex::new(0);

    /// 块列表，存放各块大小，初始为 None
    pub static ref OPT_BLOCK_LIST: Mutex<Option<Vec<u64>>> = Mutex::new(None);

    /// 用于编码和解码所需的过滤器数组，大小为 LZMA_FILTERS_MAX + 1
    pub static ref FILTERS: Mutex<[LzmaFilter; LZMA_FILTERS_MAX + 1]> =
        Mutex::new(core::array::from_fn(|_| LzmaFilter::default()));

    /// 输入缓冲区
    pub static ref IN_BUF: Mutex<IoBuf> = Mutex::new(IoBuf::new());

    /// 输出缓冲区
    pub static ref OUT_BUF: Mutex<IoBuf> = Mutex::new(IoBuf::new());

    /// 过滤器数量，零表示使用预设，默认为 0
    pub static ref FILTERS_COUNT: Mutex<u32> = Mutex::new(0);

    /// 压缩预设编号（0-9），默认为 LZMA_PRESET_DEFAULT
    pub static ref PRESET_NUMBER: Mutex<u32> = Mutex::new(LZMA_PRESET_DEFAULT);

    /// 完整性检查类型
    pub static ref CHECK: Mutex<LzmaCheck> = Mutex::new(LzmaCheck::default());

    /// 默认完整性检查标志，当使用 --check=CHECK 选项后设为 false，默认为 true
    pub static ref CHECK_DEFAULT: Mutex<bool> = Mutex::new(true);

    /// 允许存在未消费输入的标志，解码成功后生效，默认为 false
    pub static ref ALLOW_TRAILING_INPUT: Mutex<bool> = Mutex::new(false);
}
pub fn set_opt_mode(mode: OperationMode) {
    // let mut opt_mode = OPT_MODE.lock().unwrap();
    *OPT_MODE.lock().unwrap() = mode;
}
pub fn get_opt_mode() -> OperationMode {
    let opt = OPT_MODE.lock().unwrap();
    opt.clone()
}

// OPT_FORMAT: FormatType
pub fn set_opt_format(format: FormatType) {
    // let mut opt = OPT_FORMAT.lock().unwrap();
    *OPT_FORMAT.lock().unwrap() = format;
}

pub fn get_opt_format() -> FormatType {
    let opt = OPT_FORMAT.lock().unwrap();
    opt.clone()
}

// OPT_AUTO_ADJUST: bool
pub fn set_opt_auto_adjust(val: bool) {
    // let mut opt = OPT_AUTO_ADJUST.lock().unwrap();
    *OPT_AUTO_ADJUST.lock().unwrap() = val;
}

pub fn get_opt_auto_adjust() -> bool {
    let opt = OPT_AUTO_ADJUST.lock().unwrap();
    *opt
}

// OPT_SINGLE_STREAM: bool
pub fn set_opt_single_stream(val: bool) {
    // let mut opt = OPT_SINGLE_STREAM.lock().unwrap();
    *OPT_SINGLE_STREAM.lock().unwrap() = val;
}

pub fn get_opt_single_stream() -> bool {
    let opt = OPT_SINGLE_STREAM.lock().unwrap();
    *opt
}

// OPT_BLOCK_SIZE: u64
pub fn set_opt_block_size(size: u64) {
    // let mut opt = OPT_BLOCK_SIZE.lock().unwrap();
    *OPT_BLOCK_SIZE.lock().unwrap() = size;
}

pub fn get_opt_block_size() -> u64 {
    let opt = OPT_BLOCK_SIZE.lock().unwrap();
    *opt
}

// OPT_BLOCK_LIST: Option<Vec<u64>>
pub fn set_opt_block_list(list: Vec<u64>) {
    // let mut opt = OPT_BLOCK_LIST.lock().unwrap();
    *OPT_BLOCK_LIST.lock().unwrap() = Some(list);
}

pub fn get_opt_block_list() -> Option<Vec<u64>> {
    let opt = OPT_BLOCK_LIST.lock().unwrap();
    opt.clone()
}

pub fn get_opt_flush_timeout() -> u64 {
    let timeout = OPT_FLUSH_TIMEOUT.lock().unwrap();
    *timeout
}
// FILTERS: [LzmaFilter; LZMA_FILTERS_MAX + 1]
pub fn set_filters(filters: [LzmaFilter; LZMA_FILTERS_MAX + 1]) {
    // let mut f = FILTERS.lock().unwrap();
    *FILTERS.lock().unwrap() = filters;
}

pub fn get_filters() -> [LzmaFilter; LZMA_FILTERS_MAX + 1] {
    let f = FILTERS.lock().unwrap();
    f.clone()
}

// IN_BUF: IoBuf
pub fn set_in_buf(buf: IoBuf) {
    // let mut b = IN_BUF.lock().unwrap();
    *IN_BUF.lock().unwrap() = buf;
}

pub fn get_in_buf() -> IoBuf {
    let b = IN_BUF.lock().unwrap();
    b.clone()
}

// OUT_BUF: IoBuf
pub fn set_out_buf(buf: IoBuf) {
    // let mut b = OUT_BUF.lock().unwrap();
    *OUT_BUF.lock().unwrap() = buf;
}

pub fn get_out_buf() -> IoBuf {
    let b = OUT_BUF.lock().unwrap();
    b.clone()
}

// FILTERS_COUNT: u32
pub fn set_filters_count(count: u32) {
    // let mut c = FILTERS_COUNT.lock().unwrap();
    *FILTERS_COUNT.lock().unwrap() = count;
}

pub fn get_filters_count() -> u32 {
    let c = FILTERS_COUNT.lock().unwrap();
    *c
}

// PRESET_NUMBER: u32
pub fn set_preset_number(num: u32) {
    // let mut n = PRESET_NUMBER.lock().unwrap();
    *PRESET_NUMBER.lock().unwrap() = num;
}

pub fn get_preset_number() -> u32 {
    let n = PRESET_NUMBER.lock().unwrap();
    *n
}

// CHECK: LzmaCheck
pub fn set_check(check: LzmaCheck) {
    // let mut c = CHECK.lock().unwrap();
    *CHECK.lock().unwrap() = check;
}

pub fn get_check() -> LzmaCheck {
    let c = CHECK.lock().unwrap();
    c.clone()
}

// CHECK_DEFAULT: bool
pub fn set_check_default(val: bool) {
    // let mut c = CHECK_DEFAULT.lock().unwrap();
    *CHECK_DEFAULT.lock().unwrap() = val;
}

pub fn get_check_default() -> bool {
    let c = CHECK_DEFAULT.lock().unwrap();
    *c
}

// ALLOW_TRAILING_INPUT: bool
pub fn set_allow_trailing_input(val: bool) {
    // let mut a = ALLOW_TRAILING_INPUT.lock().unwrap();
    *ALLOW_TRAILING_INPUT.lock().unwrap() = val;
}

pub fn get_allow_trailing_input() -> bool {
    let a = ALLOW_TRAILING_INPUT.lock().unwrap();
    *a
}

/// 设置完整性检查类型 new_check，并置 check_default 为 false
pub fn coder_set_check(new_check: LzmaCheck) {
    *CHECK.lock().unwrap() = new_check;
    *CHECK_DEFAULT.lock().unwrap() = false;
}

/// 忘记现有过滤器链：释放所有已设置过滤器的 options，并将过滤器数量归零
fn forget_filter_chain() {
    let mut count = FILTERS_COUNT.lock().unwrap();
    let mut filters = FILTERS.lock().unwrap();
    while *count > 0 {
        // 将过滤器 options 设置为 None（Rust 会自动释放旧的 Option）
        filters[(*count - 1) as usize].options = None;
        *count -= 1;
    }
}

/// 设置预设数值 new_preset；先清除现有预设级别部分，再设置新预设，并忘记过滤器链
pub fn coder_set_preset(new_preset: u32) {
    // let mut preset = PRESET_NUMBER.lock().unwrap();
    *PRESET_NUMBER.lock().unwrap() &= !LZMA_PRESET_LEVEL_MASK;
    *PRESET_NUMBER.lock().unwrap() |= new_preset;

    forget_filter_chain();
}

/// 启用 extreme 模式：在预设值上置 LZMA_PRESET_EXTREME 标志，并忘记过滤器链
pub fn coder_set_extreme() {
    // let mut preset = PRESET_NUMBER.lock().unwrap();
    *PRESET_NUMBER.lock().unwrap() |= LZMA_PRESET_EXTREME;
    // drop(preset);
    forget_filter_chain();
}

/// 添加过滤器到过滤器链
///
/// # 参数
/// - `id`: 过滤器的 ID
/// - `options`: 过滤器的配置选项
///
/// # 错误
/// 如果过滤器数量已达到最大值（`LZMA_FILTERS_MAX`），则报错
pub fn coder_add_filter(id: LzmaVli, options: Option<LzmaOptionsType>) {
    // let mut filters_count = FILTERS_COUNT.lock().unwrap();
    if *FILTERS_COUNT.lock().unwrap() == LZMA_FILTERS_MAX.try_into().unwrap() {
        panic!("过滤器数量已达到最大值（最多支持 {} 个）", LZMA_FILTERS_MAX);
    }

    let mut filters = FILTERS.lock().unwrap();
    filters[*FILTERS_COUNT.lock().unwrap() as usize].id = id;
    filters[*FILTERS_COUNT.lock().unwrap() as usize].options = options;

    *FILTERS_COUNT.lock().unwrap() += 1;

    // 设置自定义过滤器链会重置预设编号为默认值
    *PRESET_NUMBER.lock().unwrap() = LZMA_PRESET_DEFAULT;
}

/// 内存限制过小时的报错处理
///
/// # 参数
/// - `memory_usage`: 当前内存使用量
///
/// # 行为
/// 输出错误信息并退出程序
pub fn memlimit_too_small(memory_usage: u64) -> ! {
    eprintln!("错误：内存使用限制过低，无法满足当前过滤器设置");
    eprintln!("所需内存：{} 字节", memory_usage);
    std::process::exit(1);
}

/// 设置压缩参数
pub fn coder_set_compression_settings() {
    // 确保格式不是 LZIP
    assert!(get_opt_format() != FormatType::Lzip);

    // 如果使用默认的完整性检查，则设置为 CRC64，如果不支持则降级为 CRC32
    if *CHECK_DEFAULT.lock().unwrap() {
        *CHECK.lock().unwrap() = LzmaCheck::Crc64;
        if !lzma_check_is_supported(CHECK.lock().unwrap().clone()) {
            *CHECK.lock().unwrap() = LzmaCheck::Crc32;
        }
    }

    // 如果未设置过滤器，则使用预设值
    if get_filters_count() == 0 {
        if get_opt_format() == FormatType::Raw {
            // 在 raw 模式下使用预设值是不推荐的
            println!("警告：在 raw 模式下使用预设值是不推荐的");
            println!("警告：预设值的具体选项可能因软件版本而异");
        }

        // 获取 LZMA1 或 LZMA2 的预设值
        let mut opt_lzma = LzmaOptionsLzma::default();
        let preset_number = get_preset_number();
        if lzma_lzma_preset(&mut opt_lzma, preset_number) {
            panic!("预设值设置失败");
        }

        // 使用 LZMA2，除非格式是 LZMA1
        let mut filters = get_filters();
        filters[0].id = if get_opt_format() == FormatType::Lzma {
            LZMA_FILTER_LZMA1
        } else {
            LZMA_FILTER_LZMA2
        };
        filters[0].options = Some(LzmaOptionsType::LzmaOptionsLzma(opt_lzma));
        set_filters(filters);
        set_filters_count(1);
    }

    // 终止过滤器数组
    let mut filters = get_filters();
    filters[get_filters_count() as usize].id = u64::MAX;
    set_filters(filters);

    // 如果使用 .lzma 格式，则只允许一个 LZMA1 过滤器
    if get_opt_format() == FormatType::Lzma
        && (get_filters_count() != 1 || get_filters()[0].id != LZMA_FILTER_LZMA1)
    {
        panic!(".lzma 格式仅支持 LZMA1 过滤器");
    }

    // 如果使用 .xz 格式，则确保没有 LZMA1 过滤器
    if get_opt_format() == FormatType::Xz {
        for i in 0..get_filters_count() {
            let filters = &get_filters(); // 使用引用避免移动
            if filters[i as usize].id == LZMA_FILTER_LZMA1 {
                panic!("LZMA1 不能与 .xz 格式一起使用");
            }
        }
    }

    // // 打印选定的过滤器链
    // println!("调试：选定的过滤器链：{:?}", get_filters());

    // 如果处于压缩模式且有刷新超时设置，则检查过滤器链的兼容性
    if get_opt_mode() == OperationMode::Compress && get_opt_flush_timeout() != 0 {
        for i in 0..get_filters_count() {
            let filters = &get_filters(); // 再次使用引用
            match filters[i as usize].id {
                33 | 0x03 => (),
                _ => panic!("过滤器链与 --flush-timeout 不兼容"),
            }
        }
    }

    // 获取内存限制并计算内存使用量
    let memory_limit = hardware_memlimit_get(get_opt_mode());
    let mut memory_usage = u64::MAX;
    if get_opt_mode() == OperationMode::Compress {
        // if get_opt_format() == FormatType::Xz && hardware_threads_is_mt() {
        //     let mt_options = LzmaMt {
        //         threads: hardware_threads_get(),
        //         block_size: get_opt_block_size(),
        //         check: get_check(),
        //     };
        //     // memory_usage = lzma_stream_encoder_mt_memusage(&mt_options);
        //     if memory_usage != u64::MAX {
        //         println!("调试：使用最多 {} 个线程", mt_options.threads);
        //     }
        // } else {
        memory_usage = lzma_raw_encoder_memusage(&get_filters());
        // }
    } else {
        // 修改第406行代码，使用引用传递 filters
        memory_usage = lzma_raw_decoder_memusage(&get_filters());
    }

    if memory_usage == u64::MAX {
        panic!("不支持的过滤器链或过滤器选项");
    }

    // println!("调试：所需内存：{} 字节", memory_usage);

    if get_opt_mode() == OperationMode::Compress {
        let decmem = lzma_raw_decoder_memusage(&get_filters());
        if decmem != u64::MAX {
            message(
                MessageVerbosity::Debug,
                "调试：解压缩需要 {} MiB 内存",
                format_args!("{}", round_up_to_mib(decmem)),
            );
        }
    }

    if memory_usage <= memory_limit {
        return;
    }

    // 如果使用 raw 格式，则不会调整设置以满足内存限制
    if get_opt_format() == FormatType::Raw {
        memlimit_too_small(memory_usage);
    }

    assert!(get_opt_mode() == OperationMode::Compress);

    // if get_opt_format() == FormatType::Xz && hardware_threads_is_mt() {
    //     // 尝试减少线程数
    //     let mut mt_options = LzmaMt {
    //         threads: hardware_threads_get(),
    //         block_size: get_opt_block_size(),
    //         check: get_check(),
    //     };
    //     while mt_options.threads > 1 {
    //         mt_options.threads -= 1;
    //         memory_usage = lzma_stream_encoder_mt_memusage(&mt_options);
    //         if memory_usage == u64::MAX {
    //             panic!("内存使用量计算失败");
    //         }

    //         if memory_usage <= memory_limit {
    //             println!(
    //                 "警告：将线程数从 {} 减少到 {} 以满足内存限制 {} MiB",
    //                 hardware_threads_get(),
    //                 mt_options.threads,
    //                 round_up_to_mib(memory_limit)
    //             );
    //             return;
    //         }
    //     }

    //     if hardware_memlimit_mtenc_is_default() {
    //         println!(
    //             "警告：将线程数从 {} 减少到 1。自动内存限制 {} MiB 仍被超出。需要 {} MiB 内存。继续执行。",
    //             hardware_threads_get(),
    //             round_up_to_mib(memory_limit),
    //             round_up_to_mib(memory_usage)
    //         );
    //         return;
    //     }

    //     if !get_opt_auto_adjust() {
    //         memlimit_too_small(memory_usage);
    //     }

    //     hardware_threads_set(1);
    //     memory_usage = lzma_raw_encoder_memusage(&filters);
    //     println!(
    //         "警告：切换到单线程模式以满足内存限制 {} MiB",
    //         round_up_to_mib(memory_limit)
    //     );
    // }

    if memory_usage <= memory_limit {
        return;
    }

    // 如果 --no-adjust 被指定，则不调整 LZMA2 或 LZMA1 的字典大小
    if !get_opt_auto_adjust() {
        memlimit_too_small(memory_usage);
    }

    let mut filters = get_filters(); // 提前获取 filters
    let mut i = 0;
    while filters[i as usize].id != LZMA_FILTER_LZMA2 && filters[i as usize].id != LZMA_FILTER_LZMA1
    {
        if filters[i as usize].id == u64::MAX {
            memlimit_too_small(memory_usage);
        }
        i += 1;
    }

    let opt =
        if let Some(LzmaOptionsType::LzmaOptionsLzma(ref mut opt)) = filters[i as usize].options {
            opt
        } else {
            panic!("无效的 LZMA 选项");
        };
    let orig_dict_size = opt.dict_size;
    opt.dict_size &= !((1 << 20) - 1);
    loop {
        if opt.dict_size < (1 << 20) {
            memlimit_too_small(memory_usage);
        }

        let filters = get_filters();
        memory_usage = lzma_raw_encoder_memusage(&filters);

        if memory_usage == u64::MAX {
            panic!("内存使用量计算失败");
        }

        if memory_usage <= memory_limit {
            break;
        }

        opt.dict_size -= 1 << 20;
    }

    println!(
        "警告：将 LZMA{} 字典大小从 {} MiB 调整为 {} MiB 以满足内存限制 {} MiB",
        if filters[i as usize].id == LZMA_FILTER_LZMA2 {
            '2'
        } else {
            '1'
        },
        orig_dict_size >> 20,
        opt.dict_size >> 20,
        round_up_to_mib(memory_limit)
    );
}

/// 判断输入数据是否为 XZ 格式
fn is_format_xz() -> bool {
    // 指定魔数以兼容 EBCDIC 系统
    const MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];
    let in_buf = *IN_BUF.lock().unwrap();
    let strm = STRM.lock().unwrap();
    strm.avail_in.get() >= MAGIC.len() && in_buf.data[..MAGIC.len()] == MAGIC
}

/// 判断输入数据是否为 LZMA 格式
fn is_format_lzma() -> bool {
    // LZMA 头为 13 字节
    {
        let strm = STRM.lock().unwrap();
        if strm.avail_in.get() < 13 {
            return false;
        }
    }

    // 解码 LZMA1 属性
    let mut filter = LzmaFilter {
        id: LZMA_FILTER_LZMA1,
        options: None,
    };

    if lzma_properties_decode(&mut filter, &IN_BUF.lock().unwrap().data[..5], 5) != LzmaRet::Ok {
        return false;
    }

    // 过滤假阳性：只允许字典大小为 2^n 或 2^n + 2^(n-1) 或 UINT32_MAX
    if let Some(LzmaOptionsType::LzmaOptionsLzma(opt)) = filter.options {
        let dict_size = opt.dict_size;
        if dict_size != u32::MAX {
            let mut d = dict_size - 1;
            d |= d >> 2;
            d |= d >> 3;
            d |= d >> 4;
            d |= d >> 8;
            d |= d >> 16;
            d += 1;
            if d != dict_size || dict_size == 0 {
                return false;
            }
        }
    }

    // 过滤假阳性：假设已知未压缩大小，必须小于 256 GiB
    let mut uncompressed_size = 0u64;
    for i in 0..8 {
        uncompressed_size |= (IN_BUF.lock().unwrap().data[5 + i] as u64) << (i * 8);
    }

    if uncompressed_size != u64::MAX && uncompressed_size > (1u64 << 38) {
        return false;
    }

    true
}

/// 判断输入数据是否为 LZIP 格式
fn is_format_lzip() -> bool {
    const MAGIC: [u8; 4] = [0x4C, 0x5A, 0x49, 0x50];
    let in_buf = IN_BUF.lock().unwrap();
    let strm = STRM.lock().unwrap();
    strm.avail_in.get() >= MAGIC.len() && in_buf.data[..MAGIC.len()] == MAGIC
}

/// 初始化编码器/解码器
fn coder_init(pair: &FilePair) -> CoderInitRet {
    let mut ret = LzmaRet::ProgError;

    // 初始化允许尾部输入标志
    *ALLOW_TRAILING_INPUT.lock().unwrap() = false;

    if *OPT_MODE.lock().unwrap() == OperationMode::Compress {
        match *OPT_FORMAT.lock().unwrap() {
            FormatType::Auto => {
                // args.c 确保不会进入此分支
                panic!("自动格式不应在压缩模式下使用");
            }
            FormatType::Xz => {
                ret = lzma_stream_encoder(
                    &mut STRM.lock().unwrap(),
                    &*FILTERS.lock().unwrap(),
                    CHECK.lock().unwrap().clone(),
                );
            }
            FormatType::Lzma => {
                let filters = FILTERS.lock().unwrap();
                if let Some(LzmaOptionsType::LzmaOptionsLzma(ref opt)) = filters[0].options {
                    ret = lzma_alone_encoder(&mut STRM.lock().unwrap(), opt);
                } else {
                    panic!("无效的 LZMA 选项");
                }
            }
            FormatType::Lzip => {
                // args.c 应禁止此分支
                panic!("LZIP 格式不应在压缩模式下使用");
            }
            FormatType::Raw => {
                ret = lzma_raw_encoder(&mut STRM.lock().unwrap(), &*FILTERS.lock().unwrap());
            }
        }
    } else {
        let mut flags = 0;

        // 如果忽略检查，则设置忽略检查标志
        if *OPT_IGNORE_CHECK.lock().unwrap() {
            flags |= LZMA_IGNORE_CHECK;
        } else {
            flags |= LZMA_TELL_UNSUPPORTED_CHECK;
        }

        // 如果启用单流模式，则允许尾部输入
        if *OPT_SINGLE_STREAM.lock().unwrap() {
            *ALLOW_TRAILING_INPUT.lock().unwrap() = true;
        } else {
            flags |= LZMA_CONCATENATED;
        }

        // 使用 FORMAT_AUTO 表示未知文件格式，可能启用直通模式
        let mut init_format = FormatType::Auto;

        match *OPT_FORMAT.lock().unwrap() {
            FormatType::Auto => {
                // 优先检查 .xz 格式，因为 .lzma 检测更复杂（无魔数）
                if is_format_xz() {
                    init_format = FormatType::Xz;
                } else if is_format_lzip() {
                    init_format = FormatType::Lzip;
                } else if is_format_lzma() {
                    init_format = FormatType::Lzma;
                }
            }
            FormatType::Xz => {
                if is_format_xz() {
                    init_format = FormatType::Xz;
                }
            }
            FormatType::Lzma => {
                if is_format_lzma() {
                    init_format = FormatType::Lzma;
                }
            }
            FormatType::Lzip => {
                if is_format_lzip() {
                    init_format = FormatType::Lzip;
                }
            }
            FormatType::Raw => {
                init_format = FormatType::Raw;
            }
        }

        match init_format {
            FormatType::Auto => {
                if *OPT_MODE.lock().unwrap() == OperationMode::Decompress
                    && *OPT_STDOUT.lock().unwrap()
                    && *OPT_FORCE.lock().unwrap()
                {
                    // 这些值用于进度信息
                    STRM.lock().unwrap().total_in.set(0);
                    STRM.lock().unwrap().total_out.set(0);
                    return CoderInitRet::PassThru;
                }

                ret = LzmaRet::FormatError;
            }
            FormatType::Xz => {
                ret = lzma_stream_decoder(
                    &mut STRM.lock().unwrap(),
                    hardware_memlimit_get(OperationMode::Decompress),
                    flags,
                );
            }
            FormatType::Lzma => {
                ret = lzma_alone_decoder(
                    &mut STRM.lock().unwrap(),
                    hardware_memlimit_get(OperationMode::Decompress),
                );
            }
            FormatType::Lzip => {
                *ALLOW_TRAILING_INPUT.lock().unwrap() = true;
                ret = lzma_lzip_decoder(
                    &mut STRM.lock().unwrap(),
                    hardware_memlimit_get(OperationMode::Decompress),
                    flags,
                );
            }
            FormatType::Raw => {
                // 内存使用已在 coder_set_compression_settings 中检查
                ret = lzma_raw_decoder(&mut STRM.lock().unwrap(), &*FILTERS.lock().unwrap());
            }
        }

        if ret == LzmaRet::Ok && init_format != FormatType::Raw {
            STRM.lock().unwrap().next_out.borrow_mut().clear();
            STRM.lock().unwrap().avail_out.set(0);
            STRM.lock().unwrap().next_out_pos = 0;
            while {
                ret = lzma_code(&mut STRM.lock().unwrap(), LzmaAction::Run);
                ret == LzmaRet::UnsupportedCheck
            } {
                println!(
                    "{} {} ",
                    &pair.src_name.clone().unwrap(),
                    &message_strm(ret).to_string()
                );
            }

            if ret == LzmaRet::StreamEnd {
                ret = LzmaRet::Ok;
            }
        }
    }

    if ret != LzmaRet::Ok {
        if ret == LzmaRet::MemlimitError {
            message_mem_needed(
                MessageVerbosity::Error,
                lzma_memusage(Some(&mut STRM.lock().unwrap())),
            );
        }

        return CoderInitRet::Error;
    }

    CoderInitRet::Normal
}

/// 分割块的大小，用于单线程模式下的分块处理
///
/// # 参数
/// - `block_remaining`: 当前块剩余的大小
/// - `next_block_remaining`: 下一个块剩余的大小
/// - `list_pos`: 块列表中的当前位置
fn split_block(block_remaining: &mut u64, next_block_remaining: &mut u64, list_pos: &mut usize) {
    if *next_block_remaining > 0 {
        // 如果 `next_block_remaining` 大于 0，说明当前块已经被分割过
        assert!(!hardware_threads_is_mt()); // 确保不在多线程模式下
        assert!(*OPT_BLOCK_SIZE.lock().unwrap() > 0); // 确保块大小大于 0
        assert!(OPT_BLOCK_LIST.lock().unwrap().is_some()); // 确保块列表存在

        if *next_block_remaining > *OPT_BLOCK_SIZE.lock().unwrap() {
            // 如果下一个块剩余大小大于块大小，则继续分割
            *block_remaining = *OPT_BLOCK_SIZE.lock().unwrap();
        } else {
            // 否则，当前块是最后一个分割块
            *block_remaining = *next_block_remaining;
        }

        *next_block_remaining -= *block_remaining; // 更新下一个块剩余大小
    } else {
        // 如果 `next_block_remaining` 为 0，说明当前块已经处理完毕，移动到下一个块
        let block_list = OPT_BLOCK_LIST.lock().unwrap();
        if block_list.as_ref().unwrap()[*list_pos + 1] != 0 {
            *list_pos += 1; // 如果未到列表末尾，移动到下一个块
        }

        *block_remaining = block_list.as_ref().unwrap()[*list_pos]; // 设置当前块大小

        // 在单线程模式下，如果块大小大于预设块大小，则继续分割
        if !hardware_threads_is_mt()
            && *OPT_BLOCK_SIZE.lock().unwrap() > 0
            && *block_remaining > *OPT_BLOCK_SIZE.lock().unwrap()
        {
            *next_block_remaining = *block_remaining - *OPT_BLOCK_SIZE.lock().unwrap();
            *block_remaining = *OPT_BLOCK_SIZE.lock().unwrap();
        }
    }
}

/// 将输出缓冲区的内容写入文件
///
/// # 参数
/// - `pair`: 文件对，包含输入和输出文件
///
/// # 返回值
/// 如果写入成功，返回 `false`；如果写入失败，返回 `true`
fn coder_write_output(pair: &mut FilePair) -> bool {
    // OUT_BUF 用处不大，可以尝试删除，这里直接使用 strm.next_out 来获取数据
    unsafe {
        // if *OPT_MODE.lock().unwrap() != OperationMode::Test {
        //     if io_write(
        //         pair,
        //         &OUT_BUF.lock().unwrap(),
        //         IO_BUFFER_SIZE - STRM.lock().unwrap().avail_out.get(),
        //     ) {
        //         return true;
        //     }
        // }

        if *OPT_MODE.lock().unwrap() != OperationMode::Test {
            let mut next_out_data = STRM.lock().unwrap().next_out.borrow().to_vec();
            let written_size = IO_BUFFER_SIZE - STRM.lock().unwrap().avail_out.get();

            // println!("next_out_data: {:?}", &next_out_data[..written_size]);

            let mut io_buf = IoBuf::new();
            io_buf.data[..written_size].copy_from_slice(&next_out_data[..written_size]);

            if io_write(
                pair,
                &io_buf,
                IO_BUFFER_SIZE - STRM.lock().unwrap().avail_out.get(),
            ) {
                return true;
            }
        }

        // 重置输出缓冲区的指针和大小
        let out_buf_data = OUT_BUF.lock().unwrap().data.to_vec();
        *STRM.lock().unwrap().next_out.borrow_mut() = out_buf_data;
        STRM.lock().unwrap().avail_out.set(IO_BUFFER_SIZE);
        STRM.lock().unwrap().next_out_pos = 0;
        false
    }
}

/// 执行正常的编码/解码操作
fn coder_normal(pair: &mut FilePair) -> bool {
    // 编码器需要知道何时已经提供了所有输入。
    // 解码器在使用 LZMA_CONCATENATED 时也需要知道。
    // 需要在这里检查 src_eof，因为如果是解压缩，第一个输入块已经被读取，
    // 这可能是我们唯一会读取的块。
    let mut action = if pair.src_eof {
        LzmaAction::Finish
    } else {
        LzmaAction::Run
    };

    let mut ret: LzmaRet;
    let mut success = false; // 假设会出现问题

    // block_remaining 表示在完成当前 .xz 块之前需要编码的输入字节数。
    // 块大小通过 --block-size=SIZE 和 --block-list 设置。
    // 仅在压缩为 .xz 格式时有效。如果 block_remaining == UINT64_MAX，
    // 则只创建一个块。
    let mut block_remaining = u64::MAX;

    // next_block_remaining 用于单线程模式下，当 --block-list 中的块大于 --block-size=SIZE 时。
    let mut next_block_remaining = 0;

    // 在 opt_block_list 中的位置。如果未使用 --block-list，则未使用。
    let mut list_pos = 0;

    // 处理单线程模式下的 --block-size 和 --block-list 的第一步。
    if *OPT_MODE.lock().unwrap() == OperationMode::Compress
        && *OPT_FORMAT.lock().unwrap() == FormatType::Xz
    {
        // 在线程模式下，--block-size 不会做任何事情，
        // 因为线程编码器会负责将块分割为固定大小。
        if !hardware_threads_is_mt() && *OPT_BLOCK_SIZE.lock().unwrap() > 0 {
            block_remaining = *OPT_BLOCK_SIZE.lock().unwrap();
        }

        // 如果使用了 --block-list，则从第一个大小开始。
        // 对于线程模式，--block-size 指定编码器需要准备创建的最大块大小，
        // 而 --block-list 会同时在指定间隔处启动新块。
        // 为了保持逻辑一致，单线程模式下也这样做。
        // 输出仍然不完全相同，因为在单线程模式下，大小信息不会写入块头。
        if let Some(block_list) = &*OPT_BLOCK_LIST.lock().unwrap() {
            if block_remaining < block_list[list_pos] {
                assert!(!hardware_threads_is_mt());
                next_block_remaining = block_list[list_pos] - block_remaining;
            } else {
                block_remaining = block_list[list_pos];
            }
        }
    }

    unsafe {
        let out_buf_data = OUT_BUF.lock().unwrap().data.to_vec();
        *STRM.lock().unwrap().next_out.borrow_mut() = out_buf_data;
        STRM.lock().unwrap().avail_out.set(IO_BUFFER_SIZE);
    }

    while !*USER_ABORT.lock().unwrap() {
        if STRM.lock().unwrap().avail_in.get() == 0 && action == LzmaAction::Run {
            let read_size = io_read(
                pair,
                &mut IN_BUF.lock().unwrap(),
                block_remaining
                    .min(IO_BUFFER_SIZE.try_into().unwrap())
                    .try_into()
                    .unwrap(),
            );
            // 使用Box::leak来创建静态引用
            let in_buf_data = Box::leak(IN_BUF.lock().unwrap().data.to_vec().into_boxed_slice());
            STRM.lock().unwrap().next_in = in_buf_data;
            STRM.lock().unwrap().avail_in.set(read_size);

            if STRM.lock().unwrap().avail_in.get() == usize::MAX {
                break;
            }

            if pair.src_eof {
                action = LzmaAction::Finish;
            } else if block_remaining != u64::MAX {
                // 每处理完 opt_block_size 字节的输入后，启动一个新块。
                block_remaining -= STRM.lock().unwrap().avail_in.get() as u64;
                if block_remaining == 0 {
                    action = LzmaAction::FullBarrier;
                }
            }

            if action == LzmaAction::Run && pair.flush_needed {
                action = LzmaAction::SyncFlush;
            }
        }

        // 让 liblzma 执行实际工作。
        let action_t = action.clone();

        // println!("STRM  {:#?}", STRM.lock().unwrap().internal);
        ret = lzma_code(&mut STRM.lock().unwrap(), action_t);

        // 如果输出缓冲区已满，则写出。
        if STRM.lock().unwrap().avail_out.get() == 0 {
            if coder_write_output(pair) {
                break;
            }
        }

        if ret == LzmaRet::StreamEnd
            && (action == LzmaAction::SyncFlush || action == LzmaAction::FullBarrier)
        {
            if action == LzmaAction::SyncFlush {
                // 刷新完成。立即写出待处理的数据，以便读取端可以解压缩所有已压缩的数据。
                if coder_write_output(pair) {
                    break;
                }

                // 标记自上次刷新以来未看到任何新输入。
                pair.src_has_seen_input = false;
                pair.flush_needed = false;
            } else {
                // 在 LZMA_FULL_BARRIER 后启动一个新块。
                if OPT_BLOCK_LIST.lock().unwrap().is_none() {
                    assert!(!hardware_threads_is_mt());
                    assert!(*OPT_BLOCK_SIZE.lock().unwrap() > 0);
                    block_remaining = *OPT_BLOCK_SIZE.lock().unwrap();
                } else {
                    split_block(
                        &mut block_remaining,
                        &mut next_block_remaining,
                        &mut list_pos,
                    );
                }
            }

            // 在 LZMA_FULL_FLUSH 后启动一个新块，或在 LZMA_SYNC_FLUSH 后继续同一块。
            action = LzmaAction::Run;
        } else if ret != LzmaRet::Ok {
            // 确定返回值是否表示我们将停止编码。
            // 如果使用了 LZMA_TELL_ANY_CHECK，LZMA_NO_CHECK 也会在这里。
            let stop = ret != LzmaRet::UnsupportedCheck;

            if stop {
                // 即使出现问题，也写出剩余的字节，因为这样用户可以获得尽可能多的数据，
                // 这在尝试从损坏的文件中获取一些有用数据时可能很有用。

                // println!("stop: STRM.next_out: {:?}", STRM.lock().unwrap().next_out.borrow());
                // println!("stop: STRM.avail_out: {:?}", STRM.lock().unwrap().avail_out.get());
                if coder_write_output(pair) {
                    break;
                }
            }

            if ret == LzmaRet::StreamEnd {
                if *ALLOW_TRAILING_INPUT.lock().unwrap() {
                    io_fix_src_pos(pair, STRM.lock().unwrap().avail_in.get());
                    success = true;
                    break;
                }

                // 检查是否有尾随垃圾。
                // 这对于 LZMA_Alone 和原始流是必需的。
                // 对于 .lz 文件不这样做，因为该格式明确要求允许尾随垃圾。
                if STRM.lock().unwrap().avail_in.get() == 0 && !pair.src_eof {
                    // 尝试再读取一个字节。
                    // 希望我们不会获得更多输入，因此 pair->src_eof 变为 true。

                    STRM.lock().unwrap().avail_in.set(io_read(
                        pair,
                        &mut IN_BUF.lock().unwrap(),
                        1,
                    ));

                    if STRM.lock().unwrap().avail_in.get() == usize::MAX {
                        break;
                    }

                    assert!(
                        STRM.lock().unwrap().avail_in.get() == 0
                            || STRM.lock().unwrap().avail_in.get() == 1
                    );
                }

                if STRM.lock().unwrap().avail_in.get() == 0 {
                    assert!(pair.src_eof);
                    success = true;
                    break;
                }

                // 我们尚未到达文件末尾。
                ret = LzmaRet::DataError;
                assert!(stop);
            }

            // 如果到达这里且 stop 为 true，则表示出现问题并打印错误。
            // 否则只是警告，编码可以继续。
            if stop {
                println!("错误：{:#?}: {}", pair.src_name, message_strm(ret));
            } else {
                println!("警告：{:#?}: {}", pair.src_name, message_strm(ret));

                // 压缩时，所有可能的错误都会将 stop 设置为 true。
                assert!(*OPT_MODE.lock().unwrap() != OperationMode::Compress);
            }

            if ret == LzmaRet::MemlimitError {
                // 显示实际需要多少内存。
                message_mem_needed(MessageVerbosity::Error, unsafe {
                    lzma_memusage(Some(&mut STRM.lock().unwrap()))
                });
            }

            if stop {
                break;
            }
        }

        // 在某些条件下显示进度信息。
        message_progress_update();
    }

    success
}

/// 直通模式处理函数
/// 将输入数据直接写入输出文件，不进行压缩或解压缩
fn coder_passthru(pair: &mut FilePair) -> bool {
    while STRM.lock().unwrap().avail_in.get() != 0 {
        // 如果用户中断操作，则返回失败
        if *USER_ABORT.lock().unwrap() {
            return false;
        }

        // 将输入缓冲区的内容写入输出文件
        if io_write(
            pair,
            &IN_BUF.lock().unwrap(),
            STRM.lock().unwrap().avail_in.get(),
        ) {
            return false;
        }

        // 更新已处理的输入和输出字节数 - 使用Cell的方法
        let current_total = STRM.lock().unwrap().total_in.get();
        let avail_in = STRM.lock().unwrap().avail_in.get();
        STRM.lock()
            .unwrap()
            .total_in
            .set(current_total + avail_in as u64);

        let total_in = STRM.lock().unwrap().total_in.get();
        STRM.lock().unwrap().total_out.set(total_in);

        message_progress_update();

        STRM.lock().unwrap().avail_in.set(io_read(
            pair,
            &mut IN_BUF.lock().unwrap(),
            IO_BUFFER_SIZE,
        ));
        if STRM.lock().unwrap().avail_in.get() == usize::MAX {
            return false;
        }
    }

    true
}

/// 运行编码器/解码器
pub fn coder_run(filename: &str) {
    // 设置并打印文件名，用于进度信息
    message_filename(filename);

    // 尝试打开输入文件
    let mut pair = io_open_src(filename);
    if pair.is_none() {
        return;
    }
    let mut pair = pair.unwrap();

    // 假设操作会失败
    let mut success = false;

    if *OPT_MODE.lock().unwrap() == OperationMode::Compress {
        // 压缩模式下，初始化输入缓冲区为空
        STRM.lock().unwrap().next_in = &[];
        STRM.lock().unwrap().avail_in.set(0);
    } else {
        // 解压缩模式下，读取第一块输入数据以检测文件类型
        let read_size = io_read(&mut pair, &mut IN_BUF.lock().unwrap(), IO_BUFFER_SIZE);
        let in_buf_data = Box::leak(IN_BUF.lock().unwrap().data.to_vec().into_boxed_slice());
        STRM.lock().unwrap().next_in = in_buf_data;
        STRM.lock().unwrap().avail_in.set(read_size);
    }

    // println!("avail_in: {}", STRM.lock().unwrap().avail_in.get());
    // println!("next_in: {:?}", STRM.lock().unwrap().next_in);

    if STRM.lock().unwrap().avail_in.get() != usize::MAX {
        // 初始化编码器/解码器，检测文件格式并检查内存使用情况
        let init_ret = coder_init(&pair);

        if init_ret != CoderInitRet::Error && !*USER_ABORT.lock().unwrap() {
            // 测试模式下不打开目标文件
            let open_ret = io_open_dest(&mut pair);
            let om: OperationMode = (*OPT_MODE.lock().unwrap()).clone();
            if om == OperationMode::Test || !open_ret {
                // 记录当前时间，用于进度指示器
                mytime_set_start_time();

                // 初始化进度指示器
                let is_passthru = init_ret == CoderInitRet::PassThru;
                let in_size = if pair.src_st.st_size <= 0 {
                    0
                } else {
                    pair.src_st.st_size as u64
                };
                message_progress_start(&mut STRM.lock().unwrap(), is_passthru, in_size);

                // 执行实际的编码/解码或直通操作
                if is_passthru {
                    success = coder_passthru(&mut pair);
                } else {
                    success = coder_normal(&mut pair);
                }

                // 结束进度指示器
                message_progress_end(success);
            }
        }
    }

    // 关闭文件对，根据操作是否成功决定是否删除源文件或目标文件
    io_close(&mut pair, success);
}
