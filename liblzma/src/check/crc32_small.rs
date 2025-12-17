/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use lazy_static::lazy_static;
use std::sync::{Mutex, Once};
/// CRC32 预计算查找表
lazy_static! {
    pub static ref LZMA_CRC32_TABLE: Mutex<[[u32; 256]; 1]> = Mutex::new([[0; 256]; 1]);
}
static INIT_CRC32: Once = Once::new();

/// CRC32 生成多项式
const POLY32: u32 = 0xEDB88320;

/// 初始化 CRC32 查找表
fn crc32_init() {
    // 通过 lock 获取可变引用
    let mut table = LZMA_CRC32_TABLE.lock().unwrap();
    for b in 0..256 {
        let mut r = b as u32;
        // 计算 CRC32
        for _ in 0..8 {
            if r & 1 != 0 {
                r = (r >> 1) ^ POLY32;
            } else {
                r >>= 1;
            }
        }
        table[0][b] = r; // 安全地对全局表赋值
    }
}

/// 计算 CRC32 校验值
/// 根据提供的C语言代码实现
pub fn lzma_crc32(buf: &[u8], size: usize, mut crc: u32) -> u32 {
    // 确保 CRC32 查找表已初始化
    INIT_CRC32.call_once(|| crc32_init());

    crc = !crc;

    let mut i = 0;
    while i < size {
        // 获取当前字节，模拟C代码中的 *buf++
        let b = buf[i];
        // 使用查找表计算CRC，模拟C代码中的逻辑
        let table = LZMA_CRC32_TABLE.lock().unwrap();
        crc = table[0][(b ^ (crc as u8)) as usize] ^ (crc >> 8);
        i += 1;
    }

    !crc
}
