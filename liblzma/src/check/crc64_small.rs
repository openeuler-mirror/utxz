/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::sync::OnceLock;

/// CRC64 多项式常量，与 C 代码中 UINT64_C(0xC96C5795D7870F42) 等价
pub const POLY64: u64 = 0xC96C5795D7870F42;

/// 使用 OnceLock 安全地管理全局 CRC64 查找表
static CRC64_TABLE: OnceLock<[u64; 256]> = OnceLock::new();

/// 初始化 CRC64 查找表
/// 该函数计算 256 个字节对应的 CRC64 值，并返回一个长度为 256 的数组
pub fn crc64_init() -> [u64; 256] {
    let mut table = [0u64; 256];
    // 遍历每个可能的字节值 b (从 0 到 255)
    for b in 0..256 {
        let mut r = b as u64;
        // 对每个字节执行 8 次右移并根据最低位判断是否异或 POLY64
        for _ in 0..8 {
            if r & 1 != 0 {
                r = (r >> 1) ^ POLY64;
            } else {
                r >>= 1;
            }
        }
        table[b] = r;
    }
    table
}

/// 计算 CRC64 校验值
/// 参数：
///   - buf: 输入的字节切片
///   - size: 待处理的字节数
///   - crc: 初始 CRC64 值
///
/// 返回值：最终的 CRC64 校验值
pub fn lzma_crc64(buf: &[u8], size: usize, crc: u64) -> u64 {
    // 使用 OnceLock 获取全局查找表，如果未初始化则调用 crc64_init 初始化
    let table = CRC64_TABLE.get_or_init(|| crc64_init());

    // 根据 C 代码先取反初始的 crc 值
    let mut crc = !crc;

    // 逐字节更新 CRC 值，注意仅处理前 size 个字节
    for &b in buf.iter().take(size) {
        // 根据 C 代码中的公式:
        //   crc = crc64_table[*buf++ ^ (crc & 0xFF)] ^ (crc >> 8);
        crc = table[(b ^ (crc as u8)) as usize] ^ (crc >> 8);
    }

    // 返回最终结果前再次取反
    !crc
}
