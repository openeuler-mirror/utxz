/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

pub fn tuklib_mbstr_width(str_t: &str, bytes: usize) -> usize {
    let len: usize = str_t.len();

    if bytes != 0 {
        return bytes;
    }
    len
}
