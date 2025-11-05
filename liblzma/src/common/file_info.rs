/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::LzmaStreamFlags;

use super::{LzmaIndex, LzmaNextCoder};

use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq, Default, Clone, Copy)]
pub enum Sequence {
    #[default]
    MagicBytes,
    PaddingSeek,
    PaddingDecode,
    Footer,
    IndexInit,
    IndexDecode,
    HeaderDecode,
    HeaderCompare,
}

#[derive(Debug)]
pub struct LzmaFileInfoCoder {
    /// 当前解码阶段
    sequence: Sequence,

    /// 文件中 in[*in_pos] 的绝对位置。所有修改 *in_pos 的代码也会更新此值。
    /// seek_to_pos() 需要此值来确定我们是否需要请求应用程序为我们寻找，
    /// 或者我们是否可以通过调整 *in_pos 来在内部进行寻找。
    file_cur_pos: u64,

    /// 这指的是输入文件中感兴趣部分的绝对位置。
    /// 有时它指向特定字段的*开始*，有时指向字段的*结束*。
    /// 每个时刻的当前目标位置在注释中解释。
    file_target_pos: u64,

    /// .xz 文件的大小（来自应用程序）。
    file_size: u64,

    /// 索引解码器
    index_decoder: Box<LzmaNextCoder>,

    /// 当前正在解码的索引字段中剩余的字节数。
    index_remaining: u64,

    /// 索引解码器将在此指针中存储解码后的索引。
    this_index: Option<Arc<Mutex<LzmaIndex>>>,

    /// 当前流中的流填充量。
    stream_padding: u64,

    /// 最终的组合索引在此处收集。
    combined_index: Option<Arc<Mutex<LzmaIndex>>>,

    /// 应用程序指针，用于在成功解码后存储索引信息。
    dest_index: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,

    /// 指向 lzma_stream.seek_pos 的指针，用于返回 LZMA_SEEK_NEEDED。
    /// 当需要时，由 seek_to_pos() 设置。
    external_seek_pos: Option<u64>,

    /// 内存使用限制
    memlimit: u64,

    /// 文件开头的流标志。
    first_header_flags: LzmaStreamFlags,

    /// 当前流的流头标志。
    header_flags: LzmaStreamFlags,

    /// 当前流的流尾标志。
    footer_flags: LzmaStreamFlags,

    temp_pos: usize,
    temp_size: usize,
    temp: [u8; 8192],
}

impl Default for LzmaFileInfoCoder {
    fn default() -> Self {
        LzmaFileInfoCoder {
            sequence: Sequence::MagicBytes, // 假设 `Sequence` 已实现 `Default`
            file_cur_pos: 0,
            file_target_pos: 0,
            file_size: 0,
            index_decoder: Box::new(LzmaNextCoder::default()), // 假设 `LzmaNextCoder` 已实现 `Default`
            index_remaining: 0,
            this_index: None, // 默认为 None
            stream_padding: 0,
            combined_index: None,    // 默认为 None
            dest_index: None,        // 默认为 None
            external_seek_pos: None, // 默认为 None
            memlimit: 0,
            first_header_flags: LzmaStreamFlags::default(), // 假设 `LzmaStreamFlags` 已实现 `Default`
            header_flags: LzmaStreamFlags::default(),
            footer_flags: LzmaStreamFlags::default(),
            temp_pos: 0,
            temp_size: 0,
            temp: [0; 8192], // 初始化为零
        }
    }
}
