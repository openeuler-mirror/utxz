/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#![allow(clippy::type_complexity)]
mod arm;
pub use arm::*;

mod simple_coder;
pub use simple_coder::*;

mod simple_private;
pub use simple_private::*;

mod arm64;
pub use arm64::*;

mod armthumb;
pub use armthumb::*;
