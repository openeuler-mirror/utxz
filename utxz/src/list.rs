/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::args::{OPT_FORCE, OPT_ROBOT, OPT_STDOUT, STDIN_FILENAME};
use crate::coder::{FormatType, OperationMode, OPT_FORMAT};
use crate::file_io::{
    io_close, io_open_src, io_pread, io_read, io_seek_src, FilePair, IoBuf, IO_BUFFER_SIZE,
};
use crate::hardware::hardware_memlimit_get;
use crate::message::{
    message_bug, message_error, message_fatal, message_filename, message_mem_needed, message_strm,
    message_verbosity_get, MessageVerbosity,
};
use crate::util::{round_up_to_mib, uint64_to_nicestr, uint64_to_str, NicestrUnit};
use common::{my_min, tuklib_mbstr_width};
use lazy_static::lazy_static;
use liblzma::api::{
    LzmaAction, LzmaBlock, LzmaCheck, LzmaFilter, LzmaIndexIter, LzmaIndexIterMode, LzmaRet,
    LzmaStream, LZMA_BLOCK_HEADER_SIZE_MAX, LZMA_CHECK_ID_MAX, LZMA_FILTER_ARM64,
    LZMA_FILTER_LZMA2, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_UNKNOWN,
};
use liblzma::check::lzma_check_size;
use liblzma::common::string_conversion::lzma_str_from_filters;
use liblzma::common::{
    get_dest_index, lzma_block_compressed_size, lzma_block_header_decode, lzma_code, lzma_end,
    lzma_file_info_decoder, lzma_index_block_count, lzma_index_checks, lzma_index_end,
    lzma_index_file_size, lzma_index_iter_init, lzma_index_iter_next, lzma_index_iter_rewind,
    lzma_index_stream_count, lzma_index_uncompressed_size, lzma_memusage,
    lzma_raw_decoder_memusage, LzmaIndex,
};
use liblzma::lzma_block_header_size_decode;
use std::fmt::Write;
use std::io;
use std::sync::{Arc, Mutex};

/// 关于 .xz 文件的信息
#[derive(Debug, Clone)]
pub struct XzFileInfo {
    /// 文件中所有流的组合索引
    pub idx: Option<LzmaIndex>, // 假设 lzma_index 对应 Rust 中的 LzmaIndex 类型

    /// 流填充的总量
    pub stream_padding: u64,

    /// 目前为止的最大内存使用量
    pub memusage_max: u64,

    /// 如果所有块都已经有了压缩大小和解压缩大小字段，则为 true
    pub all_have_sizes: bool,

    /// 可以解压文件的最旧 XZ 工具版本
    pub min_version: u32,
}

impl Default for XzFileInfo {
    fn default() -> Self {
        XzFileInfo {
            idx: None,
            stream_padding: 0,
            memusage_max: 0,
            all_have_sizes: true,
            min_version: 50000002,
        }
    }
}

/// .xz Block 的信息结构体
#[derive(Debug, Clone, Default)]
pub struct BlockHeaderInfo {
    /// Block Header 的大小
    pub header_size: u32,
    /// 部分 Block Flags，作为字符串（长度3，含结尾0）
    pub flags: [u8; 3],
    /// Block 中压缩数据字段的大小
    pub compressed_size: u64, // lzma_vli 通常为 u64
    /// 解码该 Block 所需的内存
    pub memusage: u64,
    /// 该 Block 的过滤器链（人类可读字符串）
    pub filter_chain: Option<String>,
}

impl BlockHeaderInfo {
    /// 创建一个默认的 BlockHeaderInfo
    pub fn new() -> Self {
        Self {
            header_size: 0,
            flags: [0; 3],
            compressed_size: 0,
            memusage: 0,
            filter_chain: None,
        }
    }

    /// 清理 filter_chain（Rust 自动管理内存，这里仅置空）
    pub fn clear_filter_chain(&mut self) {
        self.filter_chain = None;
    }
}

/// 列名字符串数组
pub static COLON_STRS: [&str; 10] = [
    "Streams:",
    "Blocks:",
    "Compressed size:",
    "Uncompressed size:",
    "Ratio:",
    "Check:",
    "Stream Padding:",
    "Memory needed:",
    "Sizes in headers:",
    // "Minimum XZ Utils version:",
    "Number of files:",
];

/// 列宽数组，线程安全全局变量
lazy_static! {
    pub static ref COLON_STRS_FW: Mutex<[usize; 11]> = Mutex::new([0; 11]);
}

/// 校验类型名称映射
pub static CHECK_NAMES: [&str; 16] = [
    "None",
    "CRC32",
    "Unknown-2",
    "Unknown-3",
    "CRC64",
    "Unknown-5",
    "Unknown-6",
    "Unknown-7",
    "Unknown-8",
    "Unknown-9",
    "SHA-256",
    "Unknown-11",
    "Unknown-12",
    "Unknown-13",
    "Unknown-14",
    "Unknown-15",
];

/// 校验值缓冲区
lazy_static! {
    pub static ref CHECK_VALUE: Mutex<String> = Mutex::new(String::with_capacity(2 * 64 + 1)); // 假设 LZMA_CHECK_SIZE_MAX = 64
    pub static ref HEADINGS_DISPLAYED: Mutex<bool> = Mutex::new(false);
}

/// 总计统计信息结构体
#[derive(Default)]
pub struct Totals {
    pub files: u64,
    pub streams: u64,
    pub blocks: u64,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub stream_padding: u64,
    pub memusage_max: u64,
    pub checks: u32,
    pub min_version: u32,
    pub all_have_sizes: bool,
}

/// 全局 totals 变量，线程安全
lazy_static! {
    pub static ref TOTALS: Mutex<Totals> = Mutex::new(Totals {
        files: 0,
        streams: 0,
        blocks: 0,
        compressed_size: 0,
        uncompressed_size: 0,
        stream_padding: 0,
        memusage_max: 0,
        checks: 0,
        min_version: 50000002,
        all_have_sizes: true,
    });
}

/// 计算多字节字符串的显示宽度（这里简单用字节长度模拟，实际可用 unicode-width crate）
fn mbstr_width(s: &str) -> usize {
    s.chars().count()
}

/// 初始化列宽数组
pub fn init_colon_strs() {
    let mut widths = [0usize; 11];
    let mut lens = [0usize; 11];
    let mut width_max = 0;

    for (i, &s) in COLON_STRS.iter().enumerate() {
        let w = mbstr_width(s);
        let l = s.len();
        widths[i] = w;
        lens[i] = l;
        if w > width_max {
            width_max = w;
        }
    }

    // 计算每列的最终宽度
    let mut fw = COLON_STRS_FW.lock().unwrap();
    for i in 0..COLON_STRS.len() {
        fw[i] = lens[i] + width_max - widths[i];
    }
}

lazy_static! {
    pub static ref HEADINGS: Mutex<[Heading; 15]> = Mutex::new([
        Heading {
            columns: 6,
            fw: 6,
            str_: "Stream"
        },
        Heading {
            columns: 9,
            fw: 9,
            str_: "Block"
        },
        Heading {
            columns: 15,
            fw: 15,
            str_: "CompOffset"
        },
        Heading {
            columns: 15,
            fw: 15,
            str_: "UncompOffset"
        },
        Heading {
            columns: 15,
            fw: 15,
            str_: "CompSize"
        },
        Heading {
            columns: 15,
            fw: 15,
            str_: "UncompSize"
        },
        Heading {
            columns: 15,
            fw: 15,
            str_: "TotalSize"
        },
        Heading {
            columns: 5,
            fw: 5,
            str_: "Ratio"
        },
        Heading {
            columns: 10,
            fw: 10,
            str_: "Check"
        },
        Heading {
            columns: 1,
            fw: 1,
            str_: "CheckVal"
        },
        Heading {
            columns: 7,
            fw: 7,
            str_: "Padding"
        },
        Heading {
            columns: 5,
            fw: 5,
            str_: "Header"
        },
        Heading {
            columns: 2,
            fw: 2,
            str_: "Flags"
        },
        Heading {
            columns: 11,
            fw: 11,
            str_: "MemUsage"
        },
        Heading {
            columns: 10,
            fw: 10,
            str_: "Filters"
        },
    ]);
}

pub struct Heading {
    pub columns: usize,
    pub fw: usize,
    pub str_: &'static str,
}

/// 版本号转字符串
pub fn xz_ver_to_str(ver: u32) -> String {
    let mut v = ver;
    let major = v / 10000000;
    v -= major * 10000000;
    let minor = v / 10000;
    v -= minor * 10000;
    let patch = v / 10;
    v -= patch * 10;
    let stability = match v {
        0 => "alpha",
        1 => "beta",
        _ => "",
    };
    format!("{}.{}.{}{}", major, minor, patch, stability)
}

/// 解析 .xz 文件索引
pub fn parse_indexes(xfi: &mut XzFileInfo, pair: &mut FilePair) -> bool {
    // 文件为空
    if pair.src_st.st_size <= 0 {
        message_error(&format!("{:#?}: 文件为空", pair.src_name), format_args!(""));
        return true;
    }

    // 文件太小
    if pair.src_st.st_size < 2 * LZMA_STREAM_HEADER_SIZE as i64 {
        message_error(
            &format!("{:#?}: 文件太小，不是有效的 .xz 文件", pair.src_name),
            format_args!(""),
        );
        return true;
    }

    let mut buf = IoBuf {
        data: [0; IO_BUFFER_SIZE],
    };
    let mut strm = LzmaStream::default();
    let mut idx: Option<Arc<Mutex<LzmaIndex>>> = None;

    let ret = lzma_file_info_decoder(
        &mut strm,
        idx.as_ref().map(|arc| Arc::new(Mutex::new(arc.clone()))),
        hardware_memlimit_get(OperationMode::List),
        pair.src_st.st_size as u64,
    );
    if ret != LzmaRet::Ok {
        message_error(
            &format!("{:#?}: {}", pair.src_name, message_strm(ret)),
            format_args!(""),
        );
        return true;
    }

    loop {
        if strm.avail_in.get() == 0 {
            let count = io_read(pair, &mut buf, IO_BUFFER_SIZE);
            strm.avail_in.set(count);
            unsafe {
                strm.next_in = std::slice::from_raw_parts(buf.data.as_ptr(), count);
            }
            if count == usize::MAX {
                lzma_end(Some(&mut strm));
                return true;
            }
        }

        let ret = lzma_code(&mut strm, LzmaAction::Run);
        if let Some(temp) = get_dest_index(&strm) {
            idx = Some(temp.lock().unwrap().clone());
        } else {
            idx = None;
        }
        match ret {
            LzmaRet::Ok => {}
            LzmaRet::SeekNeeded => {
                assert!(strm.seek_pos.get() <= pair.src_st.st_size as u64);
                if io_seek_src(pair, strm.seek_pos.get()) {
                    lzma_end(Some(&mut strm));
                    return true;
                }
                strm.avail_in.set(0);
            }
            LzmaRet::StreamEnd => {
                lzma_end(Some(&mut strm));
                xfi.idx = idx.map(|arc| arc.lock().unwrap().clone());
                // 计算 stream_padding
                let mut iter = LzmaIndexIter::default();
                lzma_index_iter_init(&mut iter, Box::new(xfi.idx.as_mut().unwrap().clone()));
                while lzma_index_iter_next(&mut iter, LzmaIndexIterMode::Stream) {
                    xfi.stream_padding += iter.stream.padding;
                }
                return false;
            }
            _ => {
                message_error(
                    &format!("{:#?}: {:#?}", pair.src_name, message_strm(ret)),
                    format_args!(""),
                );
                if ret == LzmaRet::MemlimitError {
                    message_mem_needed(MessageVerbosity::Error, lzma_memusage(Some(&mut strm)));
                }
                lzma_end(Some(&mut strm));
                return true;
            }
        }
    }
}

/// 解析 .xz Block Header，返回 true 表示出错，false 表示成功
fn parse_block_header(
    pair: &mut FilePair,
    iter: &LzmaIndexIter,
    bhi: &mut BlockHeaderInfo,
    all_have_sizes: &mut bool,
    memusage_max: &mut u64,
    min_version: &mut u32,
) -> bool {
    // 检查缓冲区大小
    assert!(
        IO_BUFFER_SIZE >= LZMA_BLOCK_HEADER_SIZE_MAX as usize,
        "IO_BUFFER_SIZE < LZMA_BLOCK_HEADER_SIZE_MAX"
    );

    // 计算要读取的 Block Header 大小
    let size = my_min(
        iter.block.total_size - lzma_check_size(iter.stream.flags.clone().unwrap().check) as u64,
        LZMA_BLOCK_HEADER_SIZE_MAX as u64,
    );

    let mut buf = IoBuf {
        data: [0; IO_BUFFER_SIZE],
    };
    if io_pread(
        pair,
        &mut buf,
        size as usize,
        iter.block.compressed_file_offset,
    ) {
        return true;
    }

    // 检查 Block Header 是否为 0（无效块）
    if buf.data[0] == 0 {
        message_error(
            &format!("{:#?}: Block Header 无效", pair.src_name),
            format_args!(""),
        );
        return true;
    }

    // 初始化 block 结构体
    let mut filters: [LzmaFilter; 5] = std::array::from_fn(|_| LzmaFilter::default());
    let mut block = LzmaBlock::default();

    // 解析 Block Header Size
    block.header_size = lzma_block_header_size_decode!(buf.data[0]);
    if block.header_size > size as u32 {
        message_error(
            &format!("{:#?}: Block Header Size 超出范围", pair.src_name),
            format_args!(""),
        );
        return true;
    }

    // 解析 Block Header

    let header_size = block.header_size;
    match lzma_block_header_decode(&mut block, &mut buf.data[..(header_size as usize)].to_vec()) {
        LzmaRet::Ok => {}
        LzmaRet::OptionsError => {
            message_error(
                &format!(
                    "{:#?}: {}",
                    pair.src_name,
                    message_strm(LzmaRet::OptionsError)
                ),
                format_args!(""),
            );
            return true;
        }
        LzmaRet::DataError => {
            message_error(
                &format!("{:#?}: {}", pair.src_name, message_strm(LzmaRet::DataError)),
                format_args!(""),
            );
            return true;
        }
        _ => {
            message_bug();
        }
    }

    // 设置 Block Flags
    bhi.flags[0] = if block.compressed_size != LZMA_VLI_UNKNOWN {
        b'c'
    } else {
        b'-'
    };
    bhi.flags[1] = if block.uncompressed_size != LZMA_VLI_UNKNOWN {
        b'u'
    } else {
        b'-'
    };
    bhi.flags[2] = 0;

    // 检查所有 Block 是否都有大小信息
    *all_have_sizes &=
        block.compressed_size != LZMA_VLI_UNKNOWN && block.uncompressed_size != LZMA_VLI_UNKNOWN;

    // 校验或设置 block.compressed_size
    match lzma_block_compressed_size(&mut block, iter.block.unpadded_size) {
        LzmaRet::Ok => {
            if block.uncompressed_size != LZMA_VLI_UNKNOWN
                && block.uncompressed_size != iter.block.uncompressed_size
            {
                message_error(
                    &format!("{:#?}: {}", pair.src_name, message_strm(LzmaRet::DataError)),
                    format_args!(""),
                );
                return true;
            }
        }
        LzmaRet::DataError => {
            message_error(
                &format!("{:#?}: {}", pair.src_name, message_strm(LzmaRet::DataError)),
                format_args!(""),
            );
            return true;
        }
        _ => {
            message_bug();
        }
    }

    // 记录头部和压缩大小
    bhi.header_size = block.header_size as u32;
    bhi.compressed_size = block.compressed_size;

    // 计算解码内存使用量
    bhi.memusage = lzma_raw_decoder_memusage(&block.filters);
    if *memusage_max < bhi.memusage {
        *memusage_max = bhi.memusage;
    }

    // 判断最小支持版本
    if *min_version < 50040002 {
        for filter in &block.filters {
            if filter.id == LZMA_FILTER_ARM64 {
                *min_version = 50040002;
                break;
            }
        }
    }
    if *min_version < 50000022 {
        let mut i = 0;
        while i + 1 < block.filters.len() && block.filters[i + 1].id != LZMA_VLI_UNKNOWN {
            i += 1;
        }
        if block.filters[i].id == LZMA_FILTER_LZMA2 && iter.block.uncompressed_size == 0 {
            *min_version = 50000022;
        }
    }

    // 过滤器链转字符串 - 简化实现
    let mut output_str = None;

    match lzma_str_from_filters(&mut output_str, &block.filters, 0) {
        LzmaRet::Ok => bhi.filter_chain = output_str,
        ret => {
            message_error(
                &format!("{:#?}: {}", pair.src_name, message_strm(ret)),
                format_args!(""),
            );
            return true;
        }
    }

    false
}

/// 解析校验值，返回 true 表示出错，false 表示成功
fn parse_check_value(pair: &mut FilePair, iter: &LzmaIndexIter) -> bool {
    // 如果没有完整性校验，直接返回 "---"
    if iter.stream.flags.clone().unwrap().check == LzmaCheck::None {
        let mut check_value = CHECK_VALUE.lock().unwrap();
        check_value.clear();
        check_value.push_str("---");
        return false;
    }

    // 定位并读取 Check 字段
    let size = lzma_check_size(iter.stream.flags.clone().unwrap().check);
    let offset = iter.block.compressed_file_offset + iter.block.total_size as u64 - size as u64;
    let mut buf = IoBuf {
        data: [0; IO_BUFFER_SIZE],
    };
    if io_pread(pair, &mut buf, size as usize, offset) {
        return true;
    }

    let mut check_value = CHECK_VALUE.lock().unwrap();
    check_value.clear();

    // CRC32/CRC64 小端序，其他按十六进制输出
    if size == 4 {
        let val = u32::from_le_bytes([buf.data[0], buf.data[1], buf.data[2], buf.data[3]]);
        write!(check_value, "{:08x}", val).unwrap();
    } else if size == 8 {
        let val = u64::from_le_bytes([
            buf.data[0],
            buf.data[1],
            buf.data[2],
            buf.data[3],
            buf.data[4],
            buf.data[5],
            buf.data[6],
            buf.data[7],
        ]);
        write!(check_value, "{:016x}", val).unwrap();
    } else {
        for i in 0..size {
            write!(check_value, "{:02x}", buf.data[i as usize]).unwrap();
        }
    }

    false
}

/// 解析 Block 详细信息，返回 true 表示出错，false 表示成功
fn parse_details(
    pair: &mut FilePair,
    iter: &mut LzmaIndexIter,
    bhi: &mut BlockHeaderInfo,
    all_have_sizes: &mut bool,
    memusage_max: &mut u64,
    min_version: &mut u32,
) -> bool {
    if parse_block_header(pair, iter, bhi, all_have_sizes, memusage_max, min_version) {
        return true;
    }
    if parse_check_value(pair, iter) {
        return true;
    }
    false
}

/// 获取压缩比字符串
fn get_ratio(compressed_size: u64, uncompressed_size: u64) -> &'static str {
    if uncompressed_size == 0 {
        return "---";
    }
    let ratio = compressed_size as f64 / uncompressed_size as f64;
    if ratio > 9.999 {
        return "---";
    }
    // 用静态缓冲区存储结果
    thread_local! {
        static RATIO_BUF: std::cell::RefCell<String> = std::cell::RefCell::new(String::with_capacity(16));
    }
    RATIO_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        buf.clear();
        write!(buf, "{:.3}", ratio).unwrap();
        Box::leak(buf.clone().into_boxed_str())
    })
}

/// 获取逗号分隔的校验类型名称列表
fn get_check_names(checks: u32, space_after_comma: bool) -> String {
    let mut checks = checks;
    if checks == 0 {
        checks = 1;
    }
    let sep = if space_after_comma { ", " } else { "," };
    let mut result = String::new();
    let mut comma = false;
    for i in 0..=LZMA_CHECK_ID_MAX {
        if checks & (1 << i) != 0 {
            if comma {
                result.push_str(sep);
            }
            let name = if *OPT_ROBOT.lock().unwrap() {
                CHECK_NAMES[i as usize]
            } else {
                CHECK_NAMES[i as usize]
            };
            result.push_str(name);
            comma = true;
        }
    }
    result
}

/// 打印 .xz 文件的基本信息表头和一行摘要
fn print_info_basic(xfi: &XzFileInfo, pair: &FilePair) -> bool {
    // 静态变量，确保表头只打印一次
    {
        let mut displayed = HEADINGS_DISPLAYED.lock().unwrap();
        if !*displayed {
            *displayed = true;
            // 打印表头
            println!("Strms  Blocks   Compressed Uncompressed  Ratio  Check   Filename");
        }
    }

    // 获取校验类型名称
    let checks = get_check_names(lzma_index_checks(xfi.idx.as_ref().unwrap()), false);

    // 构造各列字符串
    let binding = pair.src_name.clone();
    let cols = [
        uint64_to_str(
            lzma_index_stream_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
            0,
        ),
        uint64_to_str(
            lzma_index_block_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
            1,
        ),
        uint64_to_nicestr(
            lzma_index_file_size(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
            NicestrUnit::B,
            NicestrUnit::TiB,
            false,
            2,
        ),
        uint64_to_nicestr(
            lzma_index_uncompressed_size(xfi.idx.as_ref().unwrap()),
            NicestrUnit::B,
            NicestrUnit::TiB,
            false,
            3,
        ),
        get_ratio(
            lzma_index_file_size(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
            lzma_index_uncompressed_size(xfi.idx.as_ref().unwrap()),
        ),
        &checks,
        binding.as_ref().unwrap(),
    ];

    // 打印一行
    println!(
        "{:>5} {:>7}  {:>11}  {:>11}  {:>5}  {:<7} {}",
        cols[0], cols[1], cols[2], cols[3], cols[4], cols[5], cols[6]
    );

    false
}

/// 打印详细统计信息
fn print_adv_helper(
    stream_count: u64,
    block_count: u64,
    compressed_size: u64,
    uncompressed_size: u64,
    checks: u32,
    stream_padding: u64,
) {
    let checks_str = get_check_names(checks, true);

    println!("  {:<10} {}", COLON_STRS[0], uint64_to_str(stream_count, 0));
    println!("  {:<10} {}", COLON_STRS[1], uint64_to_str(block_count, 0));
    println!(
        "  {:<16} {}",
        COLON_STRS[2],
        uint64_to_nicestr(compressed_size, NicestrUnit::B, NicestrUnit::TiB, true, 0)
    );
    println!(
        "  {:<16} {}",
        COLON_STRS[3],
        uint64_to_nicestr(uncompressed_size, NicestrUnit::B, NicestrUnit::TiB, true, 0)
    );
    println!(
        "  {:<10} {}",
        COLON_STRS[4],
        get_ratio(compressed_size, uncompressed_size)
    );
    println!("  {:<10} {}", COLON_STRS[5], checks_str);
    println!(
        "  {:<16} {}",
        COLON_STRS[6],
        uint64_to_nicestr(stream_padding, NicestrUnit::B, NicestrUnit::TiB, true, 0)
    );
}

macro_rules! HEADING_STR {
    ($num:expr) => {
        HEADINGS[$num].fw, (heHEADINGSadings[$num].str)
    };
}

pub const HEADING_STREAM: usize = 0;
pub const HEADING_BLOCK: usize = 1;
pub const HEADING_BLOCKS: usize = 2;
pub const HEADING_COMPOFFSET: usize = 3;
pub const HEADING_UNCOMPOFFSET: usize = 4;
pub const HEADING_COMPSIZE: usize = 5;
pub const HEADING_UNCOMPSIZE: usize = 6;
pub const HEADING_TOTALSIZE: usize = 7;
pub const HEADING_RATIO: usize = 8;
pub const HEADING_CHECK: usize = 9;
pub const HEADING_CHECKVAL: usize = 10;
pub const HEADING_PADDING: usize = 11;
pub const HEADING_HEADERSIZE: usize = 12;
pub const HEADING_HEADERFLAGS: usize = 13;
pub const HEADING_MEMUSAGE: usize = 14;
pub const HEADING_FILTERS: usize = 15;

/// 打印 .xz 文件详细信息
fn print_info_adv(xfi: &mut XzFileInfo, pair: &mut FilePair) -> bool {
    // 打印总览信息
    print_adv_helper(
        lzma_index_stream_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
        lzma_index_block_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
        lzma_index_file_size(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
        lzma_index_uncompressed_size(xfi.idx.as_ref().unwrap()),
        lzma_index_checks(xfi.idx.as_ref().unwrap()),
        xfi.stream_padding,
    );

    // 计算最大校验值长度
    let mut check_max = 0usize;
    let headings = HEADINGS.lock().unwrap();
    // 打印流信息表头
    println!(
        "  {}\n    {:<width1$} , {:<width2$}, {:<width3$} {:>width4$} {:>width5$} {:>width6$} {:>width7$} {:>width8$} {:>width9$}",
        COLON_STRS[0],
        headings[HEADING_STREAM].str_,
        headings[HEADING_BLOCK].str_,
        headings[HEADING_COMPOFFSET].str_,
        headings[HEADING_UNCOMPOFFSET].str_,
        headings[HEADING_COMPSIZE].str_,
        headings[HEADING_UNCOMPSIZE].str_,
        headings[HEADING_RATIO].str_,
        headings[HEADING_CHECK].str_,
        headings[HEADING_PADDING].str_,
        width1 = headings[HEADING_STREAM].fw,
        width2 = headings[HEADING_BLOCK].fw,
        width3 = headings[HEADING_COMPOFFSET].fw,
        width4 = headings[HEADING_UNCOMPOFFSET].fw,
        width5 = headings[HEADING_TOTALSIZE].fw,
        width6 = headings[HEADING_UNCOMPSIZE].fw,
        width7 = headings[HEADING_RATIO].fw,
        width8 = headings[HEADING_CHECK].fw,
        width9 = headings[HEADING_PADDING].fw,
    );

    // 遍历所有流
    let mut iter = LzmaIndexIter::default();
    lzma_index_iter_init(&mut iter, Box::new(xfi.idx.as_mut().unwrap().clone()));
    while lzma_index_iter_next(&mut iter, LzmaIndexIterMode::Stream) {
        let cols1 = [
            uint64_to_str(iter.stream.number, 0),
            uint64_to_str(iter.stream.block_count, 1),
            uint64_to_str(iter.stream.compressed_offset, 2),
            uint64_to_str(iter.stream.uncompressed_offset, 3),
        ];
        print!(
            "    {:>6} {:>9} {:>15} {:>15} ",
            cols1[0], cols1[1], cols1[2], cols1[3]
        );

        let cols2 = [
            uint64_to_str(iter.stream.compressed_size, 0),
            uint64_to_str(iter.stream.uncompressed_size, 1),
            get_ratio(iter.stream.compressed_size, iter.stream.uncompressed_size),
            CHECK_NAMES[iter.stream.flags.clone().unwrap().check as usize],
            uint64_to_str(iter.stream.padding, 2),
        ];
        println!(
            "{:>15} {:>15}  {:>5}  {:<10} {:>7}",
            cols2[0], cols2[1], cols2[2], cols2[3], cols2[4]
        );

        // 更新最大校验值长度
        let check_size = lzma_check_size(iter.stream.flags.clone().unwrap().check);
        if check_size > check_max as u32 {
            check_max = check_size as usize;
        }
    }

    let detailed = message_verbosity_get() >= MessageVerbosity::Debug;

    // 如果有块，打印块信息
    if lzma_index_block_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))) > 0 {
        let headings = HEADINGS.lock().unwrap();
        let checkval_width = std::cmp::max(headings[9].columns, 2 * check_max);

        // 打印块表头
        let w = if detailed {
            headings[HEADING_BLOCKS].columns
        } else {
            1
        };
        print!(
            " {:<width1$} , {:<width2$}, {:<width3$} {:>width4$} {:>width5$} {:>width6$} {:>width7$} {:>width8$} {:>width9$}",
            COLON_STRS[1],
            headings[HEADING_STREAM].str_,
            headings[HEADING_BLOCK].str_,
            headings[HEADING_COMPOFFSET].str_,
            headings[HEADING_UNCOMPOFFSET].str_,
            headings[HEADING_TOTALSIZE].str_,
            headings[HEADING_UNCOMPSIZE].str_,
            headings[HEADING_RATIO].str_,
            headings[HEADING_CHECK].str_,
                width1 = checkval_width,
                width2 = headings[HEADING_STREAM].fw,
                width3 = headings[HEADING_BLOCK].fw,
                width4 = headings[HEADING_COMPOFFSET].fw,
                width5 = headings[HEADING_UNCOMPOFFSET].fw,
                width6 = headings[HEADING_TOTALSIZE].fw,
                width7 = headings[HEADING_UNCOMPSIZE].fw,
                width8 = headings[HEADING_RATIO].fw,
                width9 = w

        );
        if detailed {
            print!(
                " {:<width1$} , {:<width2$}, {:<width3$} {:>width4$} {:>width5$} ",
                headings[HEADING_CHECKVAL].str_,
                headings[HEADING_HEADERSIZE].str_,
                headings[HEADING_HEADERFLAGS].str_,
                headings[HEADING_COMPSIZE].str_,
                headings[HEADING_MEMUSAGE].str_,
                width1 = headings[HEADING_CHECKVAL].fw + checkval_width
                    - headings[HEADING_CHECKVAL].columns,
                width2 = headings[HEADING_HEADERSIZE].fw,
                width3 = headings[HEADING_HEADERFLAGS].fw,
                width4 = headings[HEADING_COMPSIZE].fw,
                width5 = headings[HEADING_MEMUSAGE].fw,
            );
        }
        println!();

        // 遍历所有块
        lzma_index_iter_init(&mut iter, Box::new(xfi.idx.as_mut().unwrap().clone()));
        while lzma_index_iter_next(&mut iter, LzmaIndexIterMode::Block) {
            let mut bhi = BlockHeaderInfo::default();
            if detailed
                && parse_details(
                    pair,
                    &mut iter,
                    &mut bhi,
                    &mut xfi.all_have_sizes,
                    &mut xfi.memusage_max,
                    &mut xfi.min_version,
                )
            {
                return true;
            }

            let cols1 = [
                uint64_to_str(iter.stream.number, 0),
                uint64_to_str(iter.block.number_in_stream, 1),
                uint64_to_str(iter.block.compressed_file_offset, 2),
                uint64_to_str(iter.block.uncompressed_file_offset, 3),
            ];
            print!(
                "    {:>6} {:>9} {:>15} {:>15} ",
                cols1[0], cols1[1], cols1[2], cols1[3]
            );

            let cols2 = [
                uint64_to_str(iter.block.total_size, 0),
                uint64_to_str(iter.block.uncompressed_size, 1),
                get_ratio(iter.block.total_size, iter.block.uncompressed_size),
                (CHECK_NAMES[iter.stream.flags.clone().unwrap().check as usize]),
            ];
            print!(
                "{:>15} {:>15}  {:>5}  {:<10}",
                cols2[0], cols2[1], cols2[2], cols2[3]
            );

            if detailed {
                let compressed_size = iter.block.unpadded_size
                    - bhi.header_size as u64
                    - lzma_check_size(iter.stream.flags.clone().unwrap().check) as u64;
                let cols3: [String; 6] = [
                    CHECK_VALUE.lock().unwrap().clone(),
                    uint64_to_str(bhi.header_size as u64, 0).to_string(),
                    std::str::from_utf8(&bhi.flags).unwrap_or("").to_string(),
                    uint64_to_str(compressed_size, 1).to_string(),
                    uint64_to_str(round_up_to_mib(bhi.memusage), 2).to_string(),
                    bhi.filter_chain.as_deref().unwrap_or("").to_string(),
                ];
                print!(
                    " {:<width$} {:>5} {:<2} {:>15} {:>7} {}",
                    cols3[0],
                    cols3[1],
                    cols3[2],
                    cols3[3],
                    cols3[4],
                    cols3[5],
                    width = checkval_width
                );
            }
            println!();
        }
    }

    if detailed {
        println!(
            "  {:<10} {} MiB",
            COLON_STRS[7],
            uint64_to_str(round_up_to_mib(xfi.memusage_max), 0)
        );
        println!(
            "  {:<10} {}",
            COLON_STRS[8],
            if xfi.all_have_sizes { ("Yes") } else { ("No") }
        );
        println!(
            "{} {}",
            ("  Minimum XZ Utils version:"),
            xz_ver_to_str(xfi.min_version)
        );
    }

    false
}

/// 以“机器人”格式打印 .xz 文件信息
fn print_info_robot(xfi: &mut XzFileInfo, pair: &mut FilePair) -> bool {
    // 获取校验类型名称
    let checks = get_check_names(lzma_index_checks(xfi.idx.as_ref().unwrap()), false);

    // 打印文件名
    println!("name\t{:#?}", pair.src_name);

    // 打印文件总览信息
    println!(
        "file\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        lzma_index_stream_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
        lzma_index_block_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
        lzma_index_file_size(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
        lzma_index_uncompressed_size(xfi.idx.as_ref().unwrap()),
        get_ratio(
            lzma_index_file_size(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone()))),
            lzma_index_uncompressed_size(xfi.idx.as_ref().unwrap())
        ),
        checks,
        xfi.stream_padding
    );

    // 如果详细级别足够，打印流和块信息
    if message_verbosity_get() >= MessageVerbosity::Verbose {
        let mut iter = LzmaIndexIter::default();
        lzma_index_iter_init(&mut iter, Box::new(xfi.idx.as_mut().unwrap().clone()));

        // 打印每个流
        while lzma_index_iter_next(&mut iter, LzmaIndexIterMode::Stream) {
            let flags = iter.stream.flags.as_ref().unwrap();
            println!(
                "stream\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                iter.stream.number,
                iter.stream.block_count,
                iter.stream.compressed_offset,
                iter.stream.uncompressed_offset,
                iter.stream.compressed_size,
                iter.stream.uncompressed_size,
                get_ratio(iter.stream.compressed_size, iter.stream.uncompressed_size),
                CHECK_NAMES[flags.check.clone() as usize],
                iter.stream.padding
            );
        }

        // 重置迭代器
        lzma_index_iter_rewind(&mut iter);

        // 打印每个块
        while lzma_index_iter_next(&mut iter, LzmaIndexIterMode::Block) {
            let mut bhi = BlockHeaderInfo::default();
            let mut tmp = iter.clone();
            if message_verbosity_get() >= MessageVerbosity::Debug
                && parse_details(
                    pair,
                    &mut tmp,
                    &mut bhi,
                    &mut xfi.all_have_sizes,
                    &mut xfi.memusage_max,
                    &mut xfi.min_version,
                )
            {
                return true;
            }
            let flags = iter.stream.flags.as_ref().unwrap();
            print!(
                "block\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                iter.stream.number,
                iter.block.number_in_stream,
                iter.block.number_in_file,
                iter.block.compressed_file_offset,
                iter.block.uncompressed_file_offset,
                iter.block.total_size,
                iter.block.uncompressed_size,
                get_ratio(iter.block.total_size, iter.block.uncompressed_size),
                CHECK_NAMES[flags.check.clone() as usize]
            );
            if message_verbosity_get() >= MessageVerbosity::Debug {
                print!(
                    "\t{}\t{}\t{}\t{}\t{}\t{}",
                    CHECK_VALUE.lock().unwrap().as_str(),
                    bhi.header_size,
                    std::str::from_utf8(&bhi.flags).unwrap_or(""),
                    bhi.compressed_size,
                    bhi.memusage,
                    bhi.filter_chain.as_deref().unwrap_or("")
                );
            }
            println!();
        }
    }

    // 打印 summary 信息
    if message_verbosity_get() >= MessageVerbosity::Debug {
        println!(
            "summary\t{}\t{}\t{}",
            xfi.memusage_max,
            if xfi.all_have_sizes { "yes" } else { "no" },
            xfi.min_version
        );
    }

    false
}

/// 汇总所有文件的统计信息
fn update_totals(xfi: &XzFileInfo) {
    let mut totals = TOTALS.lock().unwrap();
    totals.files += 1;
    totals.streams +=
        lzma_index_stream_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone())));
    totals.blocks +=
        lzma_index_block_count(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone())));
    totals.compressed_size +=
        lzma_index_file_size(Arc::new(Mutex::new(xfi.idx.as_ref().unwrap().clone())));
    totals.uncompressed_size += lzma_index_uncompressed_size(xfi.idx.as_ref().unwrap());
    totals.stream_padding += xfi.stream_padding;
    totals.checks |= lzma_index_checks(xfi.idx.as_ref().unwrap());

    if totals.memusage_max < xfi.memusage_max {
        totals.memusage_max = xfi.memusage_max;
    }
    if totals.min_version < xfi.min_version {
        totals.min_version = xfi.min_version;
    }
    totals.all_have_sizes &= xfi.all_have_sizes;
}

/// 打印总览统计信息（基础模式）
fn print_totals_basic() {
    // 打印分隔线
    println!("{}", "-".repeat(79));

    // 获取校验类型名称
    let totals = TOTALS.lock().unwrap();
    let checks = get_check_names(totals.checks, false);

    // 打印总览信息
    print!(
        "{:>5} {:>7}  {:>11}  {:>11}  {:>5}  {:<7} ",
        uint64_to_str(totals.streams, 0),
        uint64_to_str(totals.blocks, 1),
        uint64_to_nicestr(
            totals.compressed_size,
            NicestrUnit::B,
            NicestrUnit::TiB,
            false,
            2
        ),
        uint64_to_nicestr(
            totals.uncompressed_size,
            NicestrUnit::B,
            NicestrUnit::TiB,
            false,
            3
        ),
        get_ratio(totals.compressed_size, totals.uncompressed_size),
        checks
    );

    // 打印文件数，支持多语言复数
    // 这里只做简单实现，实际可用 i18n crate
    println!(
        "{} {}",
        uint64_to_str(totals.files, 0),
        if totals.files == 1 { "file" } else { "files" }
    );
}

/// 打印详细统计信息
fn print_totals_adv() {
    println!();
    println!("{}", ("Totals:"));

    let totals = TOTALS.lock().unwrap();
    println!(
        "  {:<10} {}",
        COLON_STRS[9], // "Number of files:"
        uint64_to_str(totals.files, 0)
    );
    print_adv_helper(
        totals.streams,
        totals.blocks,
        totals.compressed_size,
        totals.uncompressed_size,
        totals.checks,
        totals.stream_padding,
    );

    if message_verbosity_get() >= MessageVerbosity::Debug {
        println!(
            "  {:<10} {} MiB",
            COLON_STRS[7], // "Memory needed:"
            uint64_to_str(round_up_to_mib(totals.memusage_max), 0)
        );
        println!(
            "  {:<10} {}",
            COLON_STRS[8], // "Sizes in headers:"
            if totals.all_have_sizes {
                ("Yes")
            } else {
                ("No")
            }
        );
        println!(
            "{} {}",
            ("  Minimum XZ Utils version:"),
            xz_ver_to_str(totals.min_version)
        );
    }
}

/// 打印机器人格式的统计信息
fn print_totals_robot() {
    let totals = TOTALS.lock().unwrap();
    let checks = get_check_names(totals.checks, false);

    print!(
        "totals\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        totals.streams,
        totals.blocks,
        totals.compressed_size,
        totals.uncompressed_size,
        get_ratio(totals.compressed_size, totals.uncompressed_size),
        checks,
        totals.stream_padding,
        totals.files
    );

    if message_verbosity_get() >= MessageVerbosity::Debug {
        print!(
            "\t{}\t{}\t{}",
            totals.memusage_max,
            if totals.all_have_sizes { "yes" } else { "no" },
            totals.min_version
        );
    }
    println!();
}

/// 汇总所有文件的统计信息并打印
pub fn list_totals() {
    // 机器人模式下总是打印 totals
    if *OPT_ROBOT.lock().unwrap() {
        print_totals_robot();
    } else {
        let totals = TOTALS.lock().unwrap();
        // 非机器人模式下，只有文件数大于1才打印 totals
        if totals.files > 1 {
            drop(totals); // 提前释放锁，避免后续死锁
            if message_verbosity_get() <= MessageVerbosity::Warning {
                print_totals_basic();
            } else {
                print_totals_adv();
            }
        }
    }
}

pub fn init_headings() {
    // 获取 HEADINGS 的可变引用
    let mut headings = HEADINGS.lock().unwrap();

    // 处理 Check 列：遍历所有校验类型名称，更新 Check 列的最小宽度
    for name in CHECK_NAMES.iter() {
        let w = mbstr_width(name);
        if headings[HEADING_CHECK].columns < w {
            headings[HEADING_CHECK].columns = w;
        }
    }

    // 遍历所有表头，计算每个表头的实际显示宽度
    for heading in headings.iter_mut() {
        let s = heading.str_;
        let w = mbstr_width(s);
        let len = s.len();
        if heading.columns < w {
            heading.columns = w;
        }
        heading.fw = len + heading.columns - w;
    }
}

fn init_field_widths() {
    init_colon_strs();
    init_headings();
}
/// 列出单个文件的信息
pub fn list_file(filename: &str) {
    // 只支持 .xz 格式
    if *OPT_FORMAT.lock().unwrap() != FormatType::Xz
        && *OPT_FORMAT.lock().unwrap() != FormatType::Auto
    {
        message_fatal(
            "--list 仅支持 .xz 文件 (--format=xz 或 --format=auto)",
            format_args!(""),
        );
    }

    message_filename(filename);

    if filename == STDIN_FILENAME {
        message_error("--list 不支持从标准输入读取", format_args!(""));
        return;
    }

    init_field_widths();

    // 设置全局变量，控制 io_open_src 行为
    *OPT_STDOUT.lock().unwrap() = false;
    *OPT_FORCE.lock().unwrap() = true;

    // 打开源文件
    let mut pair = match io_open_src(filename) {
        Some(p) => p,
        None => return,
    };

    // 初始化文件信息
    let mut xfi = XzFileInfo::default();

    // 解析索引
    if !parse_indexes(&mut xfi, &mut pair) {
        let fail = if *OPT_ROBOT.lock().unwrap() {
            print_info_robot(&mut xfi, &mut pair)
        } else if message_verbosity_get() <= MessageVerbosity::Warning {
            print_info_basic(&xfi, &pair)
        } else {
            print_info_adv(&mut xfi, &mut pair)
        };

        // 统计汇总（只统计未出错的文件）
        if !fail {
            update_totals(&xfi);
        }

        // 释放索引
        if let Some(mut idx) = xfi.idx.take() {}
    }

    io_close(&mut pair, false);
}
