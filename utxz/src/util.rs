/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#[allow(dead_code)]
use lazy_static::lazy_static;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, MutexGuard};

// 千位分隔符状态枚举
const UNKNOWN: u8 = 0;
const WORKS: u8 = 1;
const BROKEN: u8 = 2;

// 单位枚举
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub enum NicestrUnit {
    B,
    KiB,
    MiB,
    GiB,
    TiB,
}

lazy_static! {
    static ref THOUSAND_STATE: AtomicU8 = AtomicU8::new(UNKNOWN);
    static ref BUFS: Mutex<[String; 4]> = Mutex::new([
        String::with_capacity(128),
        String::with_capacity(128),
        String::with_capacity(128),
        String::with_capacity(128)
    ]);
}

pub fn xrealloc(ptr: Option<Box<[u8]>>, size: usize) -> Box<[u8]> {
    assert!(size > 0);
    let mut vec = match ptr {
        Some(boxed) => boxed.into_vec(),
        None => Vec::new(),
    };
    vec.resize(size, 0);
    vec.into_boxed_slice()
}

/// 复制字符串并返回一个新的 `String`
pub fn xstrdup(src: &str) -> String {
    assert!(!src.is_empty(), "源字符串不能为空");

    // 直接返回源字符串的克隆副本
    src.to_string()
}

pub fn str_to_uint64(name: &str, value: &str, min: u64, max: u64) -> u64 {
    let value = value.trim_start();

    if value == "max" {
        return max;
    }

    let mut result = 0u64;
    let mut chars = value.chars().peekable();

    // 处理数字部分
    while let Some(c) = chars.peek() {
        if !c.is_ascii_digit() {
            break;
        }
        let digit = c.to_digit(10).unwrap() as u64;

        // 溢出检查
        if result > u64::MAX / 10 {
            message_fatal(name, min, max);
        }
        result *= 10;

        if u64::MAX - digit < result {
            message_fatal(name, min, max);
        }
        result += digit;

        chars.next();
    }

    // 处理后缀
    if let Some(c) = chars.next() {
        let multiplier = match c.to_ascii_lowercase() {
            'k' => 1 << 10,
            'm' => 1 << 20,
            'g' => 1 << 30,
            _ => 0,
        };

        // 检查后缀格式
        if multiplier != 0 {
            let remaining: String = chars.collect();
            if !remaining.is_empty() && remaining != "i" && remaining != "iB" && remaining != "B" {
                message_fatal(name, min, max);
            }

            // 溢出检查
            if result > u64::MAX / multiplier {
                message_fatal(name, min, max);
            }
            result *= multiplier;
        } else {
            message_fatal(name, min, max);
        }
    }

    if result < min || result > max {
        message_fatal(name, min, max);
    }

    result
}

pub fn round_up_to_mib(n: u64) -> u64 {
    (n >> 20) + u64::from((n & ((1 << 20) - 1)) != 0)
}

fn check_thousand_sep(slot: usize, buf: &mut MutexGuard<'static, [String; 4]>) {
    // let mut buf = get_bufs(slot);
    if THOUSAND_STATE.load(Ordering::Relaxed) == UNKNOWN {
        buf[slot].clear();
        let formatted = format!("{:}", 1u32);
        buf[slot] = formatted.clone();
        let works = buf[slot] == "1";
        THOUSAND_STATE.store(if works { WORKS } else { BROKEN }, Ordering::Relaxed);
    }
}

pub fn uint64_to_str(value: u64, slot: usize) -> &'static str {
    let mut buf = get_bufs(slot);
    assert!(slot < buf.len());
    check_thousand_sep(slot, &mut buf);

    if THOUSAND_STATE.load(Ordering::Relaxed) == WORKS {
        buf[slot] = format!("{:}", value);
    } else {
        buf[slot] = format!("{}", value);
    }

    let result = Box::leak(buf[slot].clone().into_boxed_str());
    result
}

pub fn uint64_to_nicestr(
    value: u64,
    unit_min: NicestrUnit,
    unit_max: NicestrUnit,
    always_also_bytes: bool,
    slot: usize,
) -> &'static str {
    let mut buf: MutexGuard<'_, [String; 4]> = get_bufs(slot);
    assert!(slot < buf.len());
    check_thousand_sep(slot, &mut buf);

    buf[slot].clear();

    let (mut d, mut unit) =
        if (unit_min == NicestrUnit::B && value < 10000) || unit_max == NicestrUnit::B {
            (value as f64, NicestrUnit::B)
        } else {
            let mut d = value as f64;
            let mut unit = NicestrUnit::B;

            loop {
                d /= 1024.0;
                unit = match unit {
                    NicestrUnit::B => NicestrUnit::KiB,
                    NicestrUnit::KiB => NicestrUnit::MiB,
                    NicestrUnit::MiB => NicestrUnit::GiB,
                    NicestrUnit::GiB => NicestrUnit::TiB,
                    NicestrUnit::TiB => break (d, unit),
                };

                if unit >= unit_min && (d <= 9999.9 || unit >= unit_max) {
                    break (d, unit);
                }
            }
        };

    let thousand_works = THOUSAND_STATE.load(Ordering::Relaxed) == WORKS;

    if unit == NicestrUnit::B {
        if thousand_works {
            buf[slot] = format!("{:}", value);
        } else {
            buf[slot] = format!("{}", value);
        }
    } else {
        if thousand_works {
            buf[slot] = format!("{:.1$}", d, 1);
        } else {
            buf[slot] = format!("{:.1}", d);
        }
    }

    buf[slot] += match unit {
        NicestrUnit::B => " B",
        NicestrUnit::KiB => " KiB",
        NicestrUnit::MiB => " MiB",
        NicestrUnit::GiB => " GiB",
        NicestrUnit::TiB => " TiB",
    };

    if always_also_bytes && value >= 10000 {
        if thousand_works {
            buf[slot] += &format!(" ({:} B)", value);
        } else {
            buf[slot] += &format!(" ({} B)", value);
        }
    }

    let result = Box::leak(buf[slot].clone().into_boxed_str());
    result
}

fn message_fatal(name: &str, min: u64, max: u64) {
    panic!(
        "Value of the option `{}` must be in the range [{}, {}]",
        name, min, max
    );
}

pub fn is_tty_stdin() -> bool {
    let ret = atty::is(atty::Stream::Stdin);
    if ret {
        eprintln!("Compressed data cannot be read from a terminal");
    }
    ret
}

pub fn is_tty_stdout() -> bool {
    let ret = atty::is(atty::Stream::Stdout);
    if ret {
        eprintln!("Compressed data cannot be written to a terminal");
    }
    ret
}

fn get_bufs(slot: usize) -> MutexGuard<'static, [String; 4]> {
    let guard = BUFS.lock().unwrap();
    assert!(slot < guard.len(), "Slot {} out of bounds", slot);
    guard
}
