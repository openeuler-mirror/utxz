/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

pub fn memzero<T: Default>(slice: &mut [T]) {
    for item in slice.iter_mut() {
        *item = T::default();
    }
}

// pub fn my_memzero(dest: *mut libc::c_void, n: usize) {
//     unsafe {
//         libc::memset(dest, 0 as libc::c_int, n);
//     }
// }

pub fn my_min<T: Ord>(x: T, y: T) -> T {
    if x < y {
        x
    } else {
        y
    }
}

pub fn my_max<T: Ord>(x: T, y: T) -> T {
    if x > y {
        x
    } else {
        y
    }
}
