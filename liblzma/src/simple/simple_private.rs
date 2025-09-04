/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{api::LzmaFilter, common::LzmaNextCoder};

#[derive(Debug, Clone)]
pub enum SimpleType {
    X86Filter(LzmaSimpleX86), // Represents the x86-specific filter data
    // Add other filter types here if needed, e.g., LZMAFilter, etc.
    LzmaFilter(LzmaFilter),
    None,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LzmaSimpleX86 {
    pub prev_mask: u32,
    pub prev_pos: u32,
}

#[derive(Debug)]
pub struct LzmaSimpleCoder {
    /// Next filter in the chain
    pub next: Box<LzmaNextCoder>,

    /// True if the next coder in the chain has returned LZMA_STREAM_END.
    pub end_was_reached: bool,

    /// True if filter() should encode the data; false to decode.
    pub is_encoder: bool,

    /// Pointer to filter-specific function, which does the actual filtering.
    pub filter: Option<fn(&mut SimpleType, u32, bool, &mut [u8], usize) -> usize>,

    /// Pointer to filter-specific data, or None if filter doesn't need any extra data.
    pub simple: SimpleType,

    /// The lowest 32 bits of the current position in the data. Most filters need this to do conversions between absolute and relative addresses.
    pub now_pos: u32,

    /// Size of the memory allocated for the buffer.
    pub allocated: usize,

    /// Flushing position in the temporary buffer. buffer[pos] is the next byte to be copied to out[].
    pub pos: usize,

    /// buffer[filtered] is the first unfiltered byte. When pos is smaller than filtered, there is unflushed filtered data in the buffer.
    pub filtered: usize,

    /// Total number of bytes (both filtered and unfiltered) currently in the temporary buffer.
    pub size: usize,

    /// Temporary buffer
    pub buffer: Vec<u8>,
}

impl LzmaSimpleCoder {
    pub fn new(size: usize) -> Self {
        LzmaSimpleCoder {
            next: Box::new(LzmaNextCoder::default()),
            end_was_reached: false,
            is_encoder: true,         // Set to `false` for decoders
            filter: None,             // Default to x86 filter
            simple: SimpleType::None, // Initialize with None or a specific filter
            now_pos: 0,
            allocated: 0,
            pos: 0,
            filtered: 0,
            size: 0,
            buffer: Vec::with_capacity(size),
        }
    }
}
