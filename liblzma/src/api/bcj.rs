/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::LZMA_VLI_C;

use super::LzmaVli;

pub const LZMA_FILTER_SW_64: LzmaVli = LZMA_VLI_C!(0x04);
pub const LZMA_FILTER_X86: LzmaVli = LZMA_VLI_C!(0x04);
pub const LZMA_FILTER_POWERPC: LzmaVli = LZMA_VLI_C!(0x05);
pub const LZMA_FILTER_IA64: LzmaVli = LZMA_VLI_C!(0x06);
pub const LZMA_FILTER_ARM: LzmaVli = LZMA_VLI_C!(0x07);
pub const LZMA_FILTER_ARMTHUMB: LzmaVli = LZMA_VLI_C!(0x08);
pub const LZMA_FILTER_SPARC: LzmaVli = LZMA_VLI_C!(0x09);
pub const LZMA_FILTER_ARM64: LzmaVli = LZMA_VLI_C!(0x0A);

#[derive(Debug, Clone, Default)]
pub struct LzmaOptionsBcj {
    pub start_offset: u32,
}
