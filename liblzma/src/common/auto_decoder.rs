/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use super::LzmaNextCoder;

#[derive(Debug, Default)]
pub struct LzmaAutoCoder {
    pub next: Box<LzmaNextCoder>,
    pub memlimit: u64,
    pub flags: u32,
    pub sequence: Sequence,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
enum Sequence {
    #[default]
    SeqInit,
    SeqCode,
    SeqFinish,
}
