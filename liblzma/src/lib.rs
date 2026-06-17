/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(unused)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_mut)]
#![allow(dead_code)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(clippy::all)]
#![allow(ambiguous_glob_reexports)]
#![allow(private_interfaces)]

#[allow(unused_variables)]
#[allow(non_upper_case_globals)]
#[macro_use]
pub mod common;

pub mod api;
pub mod check;
pub mod delta;
pub mod lz;
pub mod lzma;

#[macro_use]
pub mod rangecoder;

pub mod simple;

use crate::api::*;
