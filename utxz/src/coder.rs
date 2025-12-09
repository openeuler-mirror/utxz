/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

/// 当前的操作模式
#[derive(Debug, PartialEq, Clone)]
#[repr(u32)]
pub enum OperationMode {
    Compress,
    Decompress,
    Test,
    List,
}
