/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(
    dead_code,
    mutable_transmutes,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    unused_assignments,
    unused_mut
)]

pub mod common;
pub mod mythread;
pub mod sysdefs;
pub mod tuklib_cpucores;
pub mod tuklib_exit;
pub mod tuklib_integer;
pub mod tuklib_mbstr_fw;
pub mod tuklib_mbstr_width;
pub mod tuklib_open_stdxxx;
pub mod tuklib_physmem;
pub mod tuklib_progname;

pub use common::*;
pub use mythread::*;
pub use sysdefs::*;
pub use tuklib_cpucores::*;
pub use tuklib_exit::*;
pub use tuklib_integer::*;
pub use tuklib_mbstr_fw::*;
pub use tuklib_mbstr_width::*;
pub use tuklib_open_stdxxx::*;
pub use tuklib_physmem::*;
pub use tuklib_progname::*;
