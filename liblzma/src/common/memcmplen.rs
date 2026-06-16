/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::{my_min, read64ne};

pub const LZMA_MEMCMPLEN_EXTRA: usize = 8;

pub fn lzma_memcmplen(buf1: &[u8], buf2: &[u8], mut len: u32, limit: u32) -> u32 {
    assert!(len <= limit);
    assert!(limit <= u32::MAX / 2);

    while len < limit {
        let buf1_tem = &buf1[len as usize..(len + 8) as usize];
        let buf2_tem = &buf2[len as usize..(len + 8) as usize];
        let a = read64ne(buf1_tem);
        let b = read64ne(buf2_tem);
        let x = a.wrapping_sub(b);
        if x != 0 {
            len += x.trailing_zeros() >> 3;
            return my_min(len, limit);
        }
        len += 8;
    }

    limit
}

/// Unsafe fast memcmplen using raw pointer arithmetic.
/// Eliminates slice bounds checks in the hot match-finding loop.
///
/// # Safety
/// - `buf1` and `buf2` must point to valid buffers with at least `limit + LZMA_MEMCMPLEN_EXTRA` bytes available.
/// - `len` must be <= `limit`.
/// - `limit` must be <= `u32::MAX / 2`.
pub unsafe fn lzma_memcmplen_unchecked(
    buf1: *const u8,
    buf2: *const u8,
    mut len: u32,
    limit: u32,
) -> u32 {
    debug_assert!(len <= limit);
    debug_assert!(limit <= u32::MAX / 2);

    while len < limit {
        let a = (buf1.add(len as usize) as *const u64).read_unaligned();
        let b = (buf2.add(len as usize) as *const u64).read_unaligned();
        let x = a ^ b;
        if x != 0 {
            let inc = x.trailing_zeros() >> 3;
            let new_len = len + inc;
            return if new_len < limit { new_len } else { limit };
        }
        len += 8;
    }
    limit
}
