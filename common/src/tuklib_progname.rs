/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::common::set_progname;

pub fn tuklib_progname_init(argv: &str) {
    // progname = argv;
    set_progname(argv);
}