/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::tuklib_cpucores;

pub fn lzma_cputhreads() -> u32 {
    tuklib_cpucores()
}
