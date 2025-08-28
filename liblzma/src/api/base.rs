/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

pub type LzmaBool = u8;
pub type LzmaReservedEnum = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LzmaRet {
    Ok = 0,
    StreamEnd = 1,
    NoCheck = 2,
    UnsupportedCheck = 3,
    GetCheck = 4,
    MemError = 5,
    MemlimitError = 6,
    FormatError = 7,
    OptionsError = 8,
    DataError = 9,
    BufError = 10,
    ProgError = 11,
    SeekNeeded = 12,
    RetInternal1 = 13,
}
