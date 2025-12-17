/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#[inline]
fn conv16le(num: u16) -> u16 {
    num
}

pub fn conv32le(num: u32) -> u32 {
    num
}

#[inline]
fn conv64le(num: u64) -> u64 {
    num
}

pub fn read64le(buf: &[u8]) -> u64 {
    let num = read64ne(buf);
    conv64le(num)
}

pub fn read64ne(buf: &[u8]) -> u64 {
    u64::from_ne_bytes(buf.try_into().unwrap())
}

#[inline]
fn write32ne(buf: &mut [u8], num: u32) {
    let bytes = num.to_ne_bytes();
    if buf.len() < 4 {
        panic!(
            "Buffer too small for u32, need at least 4 bytes but got {}",
            buf.len()
        );
    }
    buf[0..4].copy_from_slice(&bytes); // 只写入前4字节
}

// fn write16le(buf: &mut [u8], num: u16) {
//     write16ne(buf, conv16le(num));
// }
fn read32ne(buf: &[u8]) -> u32 {
    if buf.len() < 4 {
        let mut result: u32 = 0;
        for (i, &byte) in buf.iter().enumerate() {
            result |= (byte as u32) << (i * 8);
        }
        return result;
    }
    u32::from_ne_bytes(buf[..4].try_into().unwrap())
}
pub fn read32le(buf: &[u8]) -> u32 {
    let num = read32ne(buf);
    conv32le(num)
}

pub fn write32le(buf: &mut [u8], num: u32) {
    write32ne(buf, conv32le(num));
}

// fn write64le(buf: &mut [u8], num: u64) {
//     write64ne(buf, conv64le(num));
// }
