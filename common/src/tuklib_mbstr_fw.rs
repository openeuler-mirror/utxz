/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::tuklib_mbstr_width::tuklib_mbstr_width;

#[no_mangle]
pub fn tuklib_mbstr_fw(str: &str, columns_min: i32) -> i32 {
    let len: usize = 0;
    // 计算字符串的宽度（在单字节模式下，宽度等于长度）
    let width = tuklib_mbstr_width(str, len);

    // 如果宽度为 usize 的最大值（表示错误），返回 -1
    if width == usize::MAX {
        return -1;
    }

    // 如果宽度大于最小列数，返回 0
    if width > columns_min as usize {
        return 0;
    }

    // 如果宽度小于最小列数，计算需要填充的长度
    let len = str.len() + (columns_min as usize - width);

    // 返回计算后的长度
    len as i32
}
