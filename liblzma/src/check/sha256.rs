/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(non_snake_case)]
use crate::check::LzmaCheckState;

/// 右旋转32位数
pub fn rotr_32(num: u32, amount: u32) -> u32 {
    (num >> amount) | (num << (32 - amount))
}

/// 宏 S0(x): C代码： rotr_32(x ^ rotr_32(x ^ rotr_32(x,9),11),2)
pub fn S0(x: u32) -> u32 {
    rotr_32(x ^ rotr_32(x ^ rotr_32(x, 9), 11), 2)
}

/// 宏 S1(x): C代码： rotr_32(x ^ rotr_32(x ^ rotr_32(x,14),5),6)
pub fn S1(x: u32) -> u32 {
    rotr_32(x ^ rotr_32(x ^ rotr_32(x, 14), 5), 6)
}

/// 宏 s0(x): C代码： rotr_32(x ^ rotr_32(x,11),7) ^ (x >> 3)
pub fn s0(x: u32) -> u32 {
    rotr_32(x ^ rotr_32(x, 11), 7) ^ (x >> 3)
}

/// 宏 s1(x): C代码： rotr_32(x ^ rotr_32(x,2),17) ^ (x >> 10)
pub fn s1(x: u32) -> u32 {
    rotr_32(x ^ rotr_32(x, 2), 17) ^ (x >> 10)
}

/// 宏 Ch(x,y,z): z ^ (x & (y ^ z))
pub fn Ch(x: u32, y: u32, z: u32) -> u32 {
    z ^ (x & (y ^ z))
}

/// 宏 Maj(x,y,z): (x & (y ^ z)) + (y & z)
pub fn Maj(x: u32, y: u32, z: u32) -> u32 {
    (x & (y ^ z)).wrapping_add(y & z)
}

/// SHA256 64 个常量，按 C代码顺序定义
pub const SHA256_K: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5,
    0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174,
    0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA,
    0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967,
    0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85,
    0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070,
    0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
];

/// transform 函数：实现 SHA-256 变换
/// 参数 state: 长度为 8 的 u32 数组，data: 长度为 16 的 u32 数组（数据为大端序，故转换为主机字节序）
/// 使用安全 Rust 代码实现，不使用 unsafe。
pub fn transform(state: &mut [u32; 8], data: &[u32; 16]) {
    // 定义扩展数组 W，大小 64
    let mut W = [0u32; 64];
    // 将 data 的前 16 个字转换为主机字节序并存入 W[0..16]
    for i in 0..16 {
        W[i] = u32::from_be(data[i]);
    }
    // 扩展剩余 48 个字
    for i in 16..64 {
        W[i] = s1(W[i - 2])
            .wrapping_add(W[i - 7])
            .wrapping_add(s0(W[i - 15]))
            .wrapping_add(W[i - 16]);
    }

    // 将 state 拷贝到工作变量 T
    let mut T = *state;

    // 64 轮主循环
    for i in 0..64 {
        let t1 = T[7]
            .wrapping_add(S1(T[4]))
            .wrapping_add(Ch(T[4], T[5], T[6]))
            .wrapping_add(SHA256_K[i])
            .wrapping_add(W[i]);
        let t2 = S0(T[0]).wrapping_add(Maj(T[0], T[1], T[2]));
        // 更新工作变量
        T = [
            t1.wrapping_add(t2),
            T[0],
            T[1],
            T[2].wrapping_add(t1),
            T[3],
            T[4],
            T[5],
            T[6],
        ];
    }

    // 将工作变量加到 state 中
    for i in 0..8 {
        state[i] = state[i].wrapping_add(T[i]);
    }
}

/// process 函数：调用 transform 对 check 中的 SHA256 状态进行处理
pub fn process(check: &mut LzmaCheckState) {
    // 假定 check.buffer.u32 为 [u32; 16]
    transform(&mut check.state.sha256.state, &check.buffer.u32);
}

/// 初始化 SHA256 检查状态
pub fn lzma_sha256_init(check: &mut LzmaCheckState) {
    // 定义初始状态常量（SHA-256 初始哈希值）
    let s: [u32; 8] = [
        0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB,
        0x5BE0CD19,
    ];
    check.state.sha256.state = s;
    check.state.sha256.size = 0;
}

/// 更新 SHA256 检查状态，处理输入的部分数据
pub fn lzma_sha256_update(buf: &[u8], size: usize, check: &mut LzmaCheckState) {
    // 处理任意长度的数据，将输入数据拷贝到临时缓冲区
    let mut remaining = size;
    let mut input = buf;
    while remaining > 0 {
        // 计算当前缓冲区中已填充数据的起始位置（取低 6 位，即模 64）
        let copy_start = (check.state.sha256.size & 0x3F) as usize;
        // 剩余填充空间
        let mut copy_size = 64 - copy_start;
        if copy_size > remaining {
            copy_size = remaining;
        }
        // 安全复制数据到缓冲区
        check.buffer.u8[copy_start..copy_start + copy_size].copy_from_slice(&input[..copy_size]);
        input = &input[copy_size..];
        remaining -= copy_size;
        check.state.sha256.size += copy_size as u64;
        // 当缓冲区满 64 字节时，进行 SHA256 变换处理
        if (check.state.sha256.size & 0x3F) == 0 {
            process(check);
        }
    }
}

/// 完成 SHA256 检查状态，将剩余数据进行填充，并计算最终校验值
pub fn lzma_sha256_finish(check: &mut LzmaCheckState) {
    // 填充数据（与 RFC 3174 所述的 SHA-1 填充方式相同，SHA-256 使用相同方式）
    let mut pos = (check.state.sha256.size & 0x3F) as usize;
    check.buffer.u8[pos] = 0x80;
    pos += 1;
    while pos != 64 - 8 {
        if pos == 64 {
            process(check);
            pos = 0;
        }
        check.buffer.u8[pos] = 0x00;
        pos += 1;
    }
    // 将消息长度（以比特为单位）存入缓冲区的最后 8 字节
    // 使用 to_be() 将长度转换为大端格式
    check.state.sha256.size *= 8;
    check.buffer.u64[(64 - 8) / 8] = check.state.sha256.size.to_be();
    process(check);
    // 将最终状态转换为大端字节序写入缓冲区（作为最终校验值）
    for i in 0..8 {
        check.buffer.u32[i] = check.state.sha256.state[i].to_be();
    }
}
