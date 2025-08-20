/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

pub const LZMA_VERSION_MAJOR: u32 = 5;
pub const LZMA_VERSION_MINOR: u32 = 4;
pub const LZMA_VERSION_PATCH: u32 = 4;
pub const LZMA_VERSION_STABILITY: u32 = LZMA_VERSION_STABILITY_STABLE;

pub const LZMA_VERSION_STABILITY_ALPHA: u32 = 0;
pub const LZMA_VERSION_STABILITY_BETA: u32 = 1;
pub const LZMA_VERSION_STABILITY_STABLE: u32 = 2;

pub const LZMA_VERSION: u32 = LZMA_VERSION_MAJOR * 10000000
    + LZMA_VERSION_MINOR * 10000
    + LZMA_VERSION_PATCH * 10
    + LZMA_VERSION_STABILITY;

pub const LZMA_VERSION_COMMIT: &str = "";
pub const LZMA_VERSION_STABILITY_STRING: &str = "";

pub fn lzma_version_string_c_(
    major: u32,
    minor: u32,
    patch: u32,
    stability: &str,
    commit: &str,
) -> String {
    format!("{}.{}.{}{}{}", major, minor, patch, stability, commit)
}

pub fn lzma_version_string_c(
    major: u32,
    minor: u32,
    patch: u32,
    stability: &str,
    commit: &str,
) -> String {
    lzma_version_string_c_(major, minor, patch, stability, commit)
}
