/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

/// CRC32 生成多项式
const POLY32: u32 = 0xEDB88320;

/// CRC32 预计算查找表（编译时常量，无锁访问）
pub const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut b = 0usize;
    while b < 256 {
        let mut r = b as u32;
        let mut j = 0;
        while j < 8 {
            if r & 1 != 0 {
                r = (r >> 1) ^ POLY32;
            } else {
                r >>= 1;
            }
            j += 1;
        }
        table[b] = r;
        b += 1;
    }
    table
};

/// 计算 CRC32 校验值
pub fn lzma_crc32(buf: &[u8], size: usize, mut crc: u32) -> u32 {
    crc = !crc;

    for &b in buf.iter().take(size) {
        crc = CRC32_TABLE[(b ^ (crc as u8)) as usize] ^ (crc >> 8);
    }

    !crc
}
