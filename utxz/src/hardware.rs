/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::args::OPT_ROBOT;
use crate::coder::OperationMode;
use crate::message::{message_verbosity_get, MessageVerbosity};
use crate::util::{round_up_to_mib, uint64_to_str};
use common::{tuklib_exit, tuklib_mbstr_fw, tuklib_mbstr_width};
use lazy_static::lazy_static;
use liblzma::common::{lzma_cputhreads, lzma_physmem};
use std::sync::Mutex;
use std::usize;

// 定义硬件全局变量结构体
#[derive(Debug)]
struct HardwareGlobals {
    threads_max: u32,
    threads_are_automatic: bool,
    use_mt_mode_with_one_thread: bool,
    memlimit_compress: u64,
    memlimit_decompress: u64,
    memlimit_mt_default: u64,
    memlimit_mtdec: u64,
    total_ram: u64,
}

// static uint32_t threads_max = 1;
// static bool threads_are_automatic = false;
// static bool use_mt_mode_with_one_thread = false;
// static uint64_t memlimit_compress = 0;
// static uint64_t memlimit_decompress = 0;
// static uint64_t memlimit_mt_default;
// static uint64_t memlimit_mtdec;
// static uint64_t total_ram;

// 全局硬件变量，初始值同 C 代码
lazy_static! {
    static ref HARDWARE: Mutex<HardwareGlobals> = Mutex::new(HardwareGlobals {
        threads_max: 1,
        threads_are_automatic: false,
        use_mt_mode_with_one_thread: false,
        memlimit_compress: 0,
        memlimit_decompress: 0,
        memlimit_mt_default: 0,
        memlimit_mtdec: 0,
        total_ram: 0,
    });
}

// hardware_threads_set: 设置工作线程数
pub fn hardware_threads_set(n: u32) {
    let mut hw = HARDWARE.lock().unwrap();
    // 重置自动线程相关标志
    hw.threads_are_automatic = false;
    hw.use_mt_mode_with_one_thread = false;

    if n == 0 {
        // 自动决定线程数
        hw.threads_are_automatic = true;
        hw.use_mt_mode_with_one_thread = true;

        let t = lzma_cputhreads();
        hw.threads_max = if t == 0 { 1 } else { t };
    } else if n == u32::MAX {
        hw.use_mt_mode_with_one_thread = true;
        hw.threads_max = 1;
    } else {
        hw.threads_max = n;
    }
}

// hardware_threads_get: 获取当前设置的最大工作线程数
pub fn hardware_threads_get() -> u32 {
    let hw = HARDWARE.lock().unwrap();
    hw.threads_max
}

// hardware_threads_is_mt: 如果线程数大于 1 或设置了用单线程多线程模式，则返回 true
pub fn hardware_threads_is_mt() -> bool {
    let hw = HARDWARE.lock().unwrap();
    hw.threads_max > 1 || hw.use_mt_mode_with_one_thread
}

// hardware_memlimit_set: 设置内存使用限制。支持以百分比设置。
pub fn hardware_memlimit_set(
    new_memlimit: u64,
    set_compress: bool,
    set_decompress: bool,
    set_mtdec: bool,
    is_percentage: bool,
) {
    let mut hw = HARDWARE.lock().unwrap();
    let mut limit = new_memlimit;
    if is_percentage {
        assert!(new_memlimit > 0 && new_memlimit <= 100);
        // 将百分比转为绝对字节数
        limit = (new_memlimit as u32 as u64 * hw.total_ram) / 100;
    }
    if set_compress {
        hw.memlimit_compress = limit;
        // 针对 32 位系统（target_pointer_width = "32"）做处理
    }
    if set_decompress {
        hw.memlimit_decompress = limit;
    }
    if set_mtdec {
        hw.memlimit_mtdec = limit;
    }
}

// hardware_memlimit_get: 根据操作模式返回硬限制
pub fn hardware_memlimit_get(mode: OperationMode) -> u64 {
    let hw = HARDWARE.lock().unwrap();
    let memlimit = if mode == OperationMode::Compress {
        hw.memlimit_compress
    } else {
        hw.memlimit_decompress
    };
    if memlimit != 0 {
        memlimit
    } else {
        u64::MAX
    }
}

// hardware_memlimit_mtenc_get: 获取多线程压缩内存限制。如果使用默认，则返回 memlimit_mt_default
pub fn hardware_memlimit_mtenc_get() -> u64 {
    if hardware_memlimit_mtenc_is_default() {
        let hw = HARDWARE.lock().unwrap();
        hw.memlimit_mt_default
    } else {
        hardware_memlimit_get(OperationMode::Compress)
    }
}

// hardware_memlimit_mtenc_is_default: 如果 memlimit_compress 为 0 且自动线程被设定，则返回 true
pub fn hardware_memlimit_mtenc_is_default() -> bool {
    let hw = HARDWARE.lock().unwrap();
    hw.memlimit_compress == 0 && hw.threads_are_automatic
}

// hardware_memlimit_mtdec_get: 获取多线程解压的“软”内存限额
pub fn hardware_memlimit_mtdec_get() -> u64 {
    let mut hw = HARDWARE.lock().unwrap();
    let mut m = if hw.memlimit_mtdec != 0 {
        hw.memlimit_mtdec
    } else {
        hw.memlimit_mt_default
    };
    if hw.memlimit_decompress != 0 && m > hw.memlimit_decompress {
        m = hw.memlimit_decompress;
    }
    m
}

// memlimit_show: 辅助函数，用于打印单行内存限额信息
fn memlimit_show(label: &str, str_columns: usize, value: u64) {
    // 计算字段宽度
    let fw = tuklib_mbstr_fw(label, str_columns as i32);
    if value == 0 || value == u64::MAX {
        println!("  {label:<width$}  {}", label, width = fw as usize);
    } else {
        println!(
            "  {label:<width$}  {} MiB ({} B)",
            uint64_to_str(round_up_to_mib(value), 0),
            uint64_to_str(value, 1),
            label = label,
            width = fw as usize
        );
    }
}

// hardware_memlimit_show: 显示硬件信息和内存限额，并退出程序
pub fn hardware_memlimit_show() {
    let hw = HARDWARE.lock().unwrap();
    let mut cputhreads: u32 = 1;

    let t = lzma_cputhreads();
    cputhreads = if t == 0 { 1 } else { t };

    let opt_robot = *OPT_ROBOT.lock().unwrap();
    if opt_robot {
        // 以制表符分隔的格式输出
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            hw.total_ram,
            hw.memlimit_compress,
            hw.memlimit_decompress,
            hardware_memlimit_mtdec_get(),
            hw.memlimit_mt_default,
            cputhreads
        );
    } else {
        let msgs = [
            ("Amount of physical memory (RAM):"),
            ("Number of processor threads:"),
            ("Compression:"),
            ("Decompression:"),
            ("Multi-threaded decompression:"),
            ("Default for -T0:"),
        ];
        let mut width_max = 1;
        for msg in msgs.iter() {
            let w = tuklib_mbstr_width(msg, 0);

            if width_max < w {
                width_max = w;
            }
        }
        println!("{}", ("Hardware information:"));
        memlimit_show(msgs[0], width_max, hw.total_ram);
        println!(
            "  {label:<width$}  {}",
            cputhreads,
            label = msgs[1],
            width = tuklib_mbstr_fw(msgs[1], width_max as i32) as usize
        );
        println!();
        println!("{}", ("Memory usage limits:"));
        memlimit_show(msgs[2], width_max, hw.memlimit_compress);
        memlimit_show(msgs[3], width_max, hw.memlimit_decompress);
        memlimit_show(msgs[4], width_max, hardware_memlimit_mtdec_get());
        memlimit_show(msgs[5], width_max, hw.memlimit_mt_default);
    }
    tuklib_exit(
        0,
        1,
        (message_verbosity_get() != MessageVerbosity::Silent) as i32,
    )
}

const ASSUME_RAM: u64 = 128;
// hardware_init: 初始化硬件相关数据。读取物理内存并计算默认内存限额。
pub fn hardware_init() {
    let mut hw = HARDWARE.lock().unwrap();
    hw.total_ram = lzma_physmem();
    if hw.total_ram == 0 {
        hw.total_ram = ASSUME_RAM * 1024 * 1024;
    }
    hw.memlimit_mt_default = hw.total_ram / 4;
}
