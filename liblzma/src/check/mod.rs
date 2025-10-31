/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(clippy::needless_range_loop)]
mod crc32_small;
mod crc64_small;

pub use crc32_small::*;
pub use crc64_small::*;
