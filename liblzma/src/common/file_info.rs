/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_max;

use crate::{
    api::{
        LzmaAction, LzmaRet, LzmaStream, LzmaStreamFlags, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_MAX,
    },
    common::{
        index, lzma_bufcpy, lzma_index_cat, lzma_index_decoder_init, lzma_index_file_size,
        lzma_index_memused, lzma_index_stream_flags, lzma_index_stream_padding,
        lzma_index_total_size, lzma_stream_flags_compare, lzma_stream_footer_decode,
        lzma_stream_header_decode, NextCoderInitFunction,
    },
};

use super::{
    lzma_end, lzma_index_end, lzma_index_memusage, lzma_next_end, lzma_strm_init, CoderType,
    LzmaIndex, LzmaNextCoder,
};

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

/// 从 `in[*in_pos]` 复制数据到 `coder.temp`，直到 `coder.temp_pos == coder.temp_size`。
/// 这也保持 `coder.file_cur_pos` 与 `*in_pos` 同步。如果需要更多输入，则返回 `true`。
fn fill_temp(
    coder: &mut LzmaFileInfoCoder,
    in_data: &[u8],
    in_pos: &mut usize,
    in_size: usize,
) -> bool {
    let bytes_copied = lzma_bufcpy(
        in_data,
        in_pos,
        in_size,
        &mut coder.temp,
        &mut coder.temp_pos,
        coder.temp_size,
    );

    coder.file_cur_pos += bytes_copied as u64;
    coder.temp_pos < coder.temp_size
}

/// 寻找目标位置 `target_pos`，尽量通过仅修改 `in_pos` 来进行寻找。
/// 如果无法通过修改 `in_pos` 寻找，则需要外部寻找。
/// 返回 `true` 如果需要外部寻找，调用者必须返回 `LZMA_SEEK_NEEDED`。
fn seek_to_pos(
    coder: &mut LzmaFileInfoCoder,
    target_pos: u64,
    in_start: usize,
    in_pos: &mut usize,
    in_size: usize,
) -> bool {
    // 输入缓冲区不会超出文件的结尾，文件信息解码时已验证过这一点。
    assert!(coder.file_size - coder.file_cur_pos >= in_size as u64 - *in_pos as u64);

    let pos_min = coder.file_cur_pos - (*in_pos as u64 - in_start as u64);
    let pos_max = coder.file_cur_pos + (in_size as u64 - *in_pos as u64);

    let external_seek_needed: bool;

    if target_pos >= pos_min && target_pos <= pos_max {
        // 请求的位置在当前输入缓冲区内或紧随其后。
        // 在一个特殊情况下，我们会将 *in_pos 设置为 in_size，
        // 然后立即需要从应用程序获取新的输入字节。
        *in_pos += (target_pos - coder.file_cur_pos) as usize;
        external_seek_needed = false;
    } else {
        // 请求外部应用程序进行文件寻址。
        coder.external_seek_pos = Some(target_pos);
        external_seek_needed = true;

        // 标记整个输入缓冲区为已使用。这样，`lzma_stream.total_in` 将会有一个更好的估算值，
        // 尽管它仍然不是完美的，因为估算值将依赖于应用程序使用的输入缓冲区大小。
        *in_pos = in_size;
    }

    // 寻址后（无论是内部寻址还是外部寻址），当前的位置将与请求的目标位置匹配。
    coder.file_cur_pos = target_pos;

    external_seek_needed
}

/// 调用者设置 `coder.file_target_pos`，指向目标文件位置的*末尾*。
/// 该函数计算我们能从该位置向后寻址的最大距离。寻址后，`fill_temp()`
/// 可以用于将数据读取到 `coder.temp` 中。当 `fill_temp()` 完成时，
/// `coder.temp[coder.temp_size]` 将与 `coder.file_target_pos` 匹配。
///
/// 此外，还会验证 `coder.target_file_pos` 的有效性，确保我们不会寻址太远（即过于接近或超出文件开头）。
fn reverse_seek(
    coder: &mut LzmaFileInfoCoder,
    in_start: usize,
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    // 检查目标位置前是否有足够的数据，至少要包含 Stream Header 和 Stream Footer。
    // 如果没有，文件无效。
    if coder.file_target_pos < 2 * LZMA_STREAM_HEADER_SIZE as u64 {
        return LzmaRet::DataError;
    }

    coder.temp_pos = 0;

    // 特别处理文件开头的 Stream Header，因为在 SEQ_MAGIC_BYTES 中已经处理过。
    // 如果文件非常小，避免不必要的外部寻址。
    if (coder.file_target_pos - LZMA_STREAM_HEADER_SIZE as u64) < coder.temp.len() as u64 {
        coder.temp_size = (coder.file_target_pos - LZMA_STREAM_HEADER_SIZE as u64) as usize;
    } else {
        coder.temp_size = coder.temp.len();
    }

    // 保证 temp_size 至少为 LZMA_STREAM_HEADER_SIZE
    assert!(coder.temp_size >= LZMA_STREAM_HEADER_SIZE);

    // 调用 `seek_to_pos` 来进行寻址。
    if seek_to_pos(
        coder,
        coder.file_target_pos - coder.temp_size as u64,
        in_start,
        in_pos,
        in_size,
    ) {
        return LzmaRet::SeekNeeded;
    }

    LzmaRet::Ok
}

/// 获取缓冲区末尾的零字节数量。
fn get_padding_size(buf: &[u8], buf_size: usize) -> usize {
    let mut padding = 0;
    let mut index = buf_size;

    while index > 0 && buf[index - 1] == 0x00 {
        padding += 1;
        index -= 1;
    }

    padding
}

/// 当文件开头的 Stream Header 不匹配 Magic Bytes 时，使用 `LZMA_FORMAT_ERROR`
/// 来告知应用程序。但在文件的中间或结尾的其他 Stream Header/Footer 字段
/// 中，返回 `LZMA_FORMAT_ERROR` 可能会让人困惑，因为我们已经知道文件开头
/// 有一个有效的 Stream Header。因此，在这些情况下，使用此函数将 `LZMA_FORMAT_ERROR` 转换为 `LZMA_DATA_ERROR`。
fn hide_format_error(ret: LzmaRet) -> LzmaRet {
    if ret == LzmaRet::FormatError {
        LzmaRet::DataError
    } else {
        ret
    }
}

/// 调用 Index 解码器并更新 `coder.index_remaining`。
/// 这是一个单独的函数，因为输入可以直接来自应用程序，也可以来自 `coder.temp`。
fn decode_index(
    coder: &mut LzmaFileInfoCoder,

    in_data: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    update_file_cur_pos: bool,
) -> LzmaRet {
    let in_start = *in_pos;

    let mut ret = LzmaRet::Ok;
    if let Some(code) = coder.index_decoder.code {
        ret = code(
            &mut coder.index_decoder.coder.as_mut().unwrap(),
            in_data,
            in_pos,
            in_size,
            &mut Vec::new(),
            &mut 0,
            0,
            LzmaAction::Run,
        );
    }
    if let Some(CoderType::IndexDecoder(ref indexDecoder)) = coder.index_decoder.coder {
        coder.this_index = indexDecoder
            .index_ptr
            .as_ref()
            .map(|inner| inner.lock().unwrap().clone());
    }
    coder.index_remaining -= (*in_pos - in_start) as u64;

    if update_file_cur_pos {
        coder.file_cur_pos += (*in_pos - in_start) as u64;
    }

    ret
}

fn file_info_decode(
    coder_ptr: &mut CoderType,
    in_data: &[u8],
    in_pos: &mut usize,
    mut in_size: usize,
    _out: &mut [u8],
    _out_pos: &mut usize,
    _out_size: usize,
    _action: LzmaAction,
) -> LzmaRet {
    let in_start = *in_pos;
    // let mut coder = coder_ptr.downcast_mut::<LzmaFileInfoCoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::FileInfo(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // 如果调用者提供的输入超过文件末尾，则裁剪多余的字节
    assert!(coder.file_size >= coder.file_cur_pos);
    if coder.file_size - coder.file_cur_pos < (in_size - in_start) as u64 {
        in_size = in_start + (coder.file_size - coder.file_cur_pos) as usize;
    }

    loop {
        let current_sequence = coder.sequence; // 获取当前状态

        let next_sequence = match current_sequence {
            Sequence::MagicBytes => {
                if coder.file_size < LZMA_STREAM_HEADER_SIZE.try_into().unwrap() {
                    return LzmaRet::FormatError;
                }

                if fill_temp(coder, in_data, in_pos, in_size) {
                    return LzmaRet::Ok;
                }

                let ret = lzma_stream_header_decode(&mut coder.first_header_flags, &coder.temp);
                if ret != LzmaRet::Ok {
                    return ret;
                }

                if coder.file_size > LZMA_VLI_MAX || (coder.file_size & 3) != 0 {
                    return LzmaRet::DataError;
                }

                coder.file_target_pos = coder.file_size;
                Sequence::PaddingSeek
            }

            Sequence::PaddingSeek => {
                let ret = reverse_seek(coder, in_start, in_pos, in_size);
                if ret != LzmaRet::Ok {
                    return ret;
                }
                Sequence::PaddingDecode
            }

            Sequence::PaddingDecode => {
                if fill_temp(coder, in_data, in_pos, in_size) {
                    return LzmaRet::Ok;
                }

                let new_padding = get_padding_size(&coder.temp, coder.temp_size);
                coder.stream_padding += new_padding as u64;
                coder.file_target_pos -= new_padding as u64;

                if new_padding == coder.temp_size {
                    coder.sequence = Sequence::PaddingSeek;
                    continue;
                }

                if coder.stream_padding & 3 != 0 {
                    return LzmaRet::DataError;
                }

                coder.temp_size -= new_padding;
                coder.temp_pos = coder.temp_size;

                if coder.temp_size < LZMA_STREAM_HEADER_SIZE {
                    let ret = reverse_seek(coder, in_start, in_pos, in_size);
                    if ret != LzmaRet::Ok {
                        return ret;
                    }
                }
                Sequence::Footer
            }

            Sequence::Footer => {
                if fill_temp(coder, in_data, in_pos, in_size) {
                    return LzmaRet::Ok;
                }

                coder.file_target_pos -= LZMA_STREAM_HEADER_SIZE as u64;
                coder.temp_size -= LZMA_STREAM_HEADER_SIZE;

                let ret = hide_format_error(lzma_stream_footer_decode(
                    &mut coder.footer_flags,
                    &coder.temp[coder.temp_size..],
                ));
                if ret != LzmaRet::Ok {
                    return ret;
                }

                if coder.file_target_pos
                    < coder.footer_flags.backward_size + LZMA_STREAM_HEADER_SIZE as u64
                {
                    return LzmaRet::DataError;
                }

                coder.file_target_pos -= coder.footer_flags.backward_size;

                if coder.temp_size >= coder.footer_flags.backward_size as usize {
                    coder.temp_pos = coder.temp_size - coder.footer_flags.backward_size as usize;
                } else {
                    coder.temp_pos = 0;
                    coder.temp_size = 0;

                    if seek_to_pos(coder, coder.file_target_pos, in_start, in_pos, in_size) {
                        return LzmaRet::SeekNeeded;
                    }
                }
                Sequence::IndexInit
            }

            Sequence::IndexInit => {
                let mut memused = 0;
                if let Some(combined_index) = &coder.combined_index {
                    memused = lzma_index_memused(combined_index.clone());
                    assert!(memused <= coder.memlimit);
                    if memused > coder.memlimit {
                        return LzmaRet::ProgError;
                    }
                }
                {
                    let ret = lzma_index_decoder_init(
                        &mut coder.index_decoder,
                        coder
                            .this_index
                            .as_ref()
                            .map(|i| Arc::new(Mutex::new(i.clone()))),
                        coder.memlimit - memused,
                    );
                    if ret != LzmaRet::Ok {
                        return ret;
                    }
                }

                coder.index_remaining = coder.footer_flags.backward_size;
                Sequence::IndexDecode
            }

            Sequence::IndexDecode => {
                let ret = if coder.temp_size != 0 {
                    assert!(coder.temp_size - coder.temp_pos == coder.index_remaining as usize);
                    // decode_index(
                    //     coder,
                    //     allocator,
                    //     &mut coder.temp.to_vec(),
                    //     &mut coder.temp_pos,
                    //     coder.temp_size,
                    //     false,
                    // )
                    let temp = coder.temp;
                    let mut temp_pos = coder.temp_pos;
                    let temp_size = coder.temp_size;

                    decode_index(coder, &mut temp.to_vec(), &mut temp_pos, temp_size, false)
                } else {
                    let mut in_stop = in_size;
                    if in_size - *in_pos > coder.index_remaining as usize {
                        in_stop = *in_pos + coder.index_remaining as usize;
                    }
                    decode_index(coder, in_data, in_pos, in_stop, true)
                };

                match ret {
                    LzmaRet::Ok => {
                        if coder.index_remaining == 0 {
                            return LzmaRet::DataError;
                        }
                        assert!(coder.temp_size == 0);
                        return LzmaRet::Ok;
                    }
                    LzmaRet::StreamEnd => {
                        if coder.index_remaining != 0 {
                            return LzmaRet::DataError;
                        }
                    }
                    _ => return ret,
                }

                let seek_amount =
                    lzma_index_total_size(&*coder.this_index.as_ref().unwrap().lock().unwrap())
                        + LZMA_STREAM_HEADER_SIZE as u64;

                if coder.file_target_pos < seek_amount {
                    return LzmaRet::DataError;
                }

                coder.file_target_pos -= seek_amount;

                if coder.file_target_pos == 0 {
                    coder.header_flags = coder.first_header_flags.clone();
                    coder.sequence = Sequence::HeaderCompare;
                    continue;
                }

                coder.file_target_pos += LZMA_STREAM_HEADER_SIZE as u64;

                if coder.temp_size != 0
                    && coder.temp_size - coder.footer_flags.backward_size as usize
                        >= seek_amount as usize
                {
                    coder.temp_pos = coder.temp_size
                        - coder.footer_flags.backward_size as usize
                        - seek_amount as usize
                        + LZMA_STREAM_HEADER_SIZE;
                    coder.temp_size = coder.temp_pos;
                } else {
                    let ret = reverse_seek(coder, in_start, in_pos, in_size);
                    if ret != LzmaRet::Ok {
                        return ret;
                    }
                }
                Sequence::HeaderDecode
            }

            Sequence::HeaderDecode => {
                if fill_temp(coder, in_data, in_pos, in_size) {
                    return LzmaRet::Ok;
                }

                coder.file_target_pos -= LZMA_STREAM_HEADER_SIZE as u64;
                coder.temp_size -= LZMA_STREAM_HEADER_SIZE;
                coder.temp_pos = coder.temp_size;

                let ret = hide_format_error(lzma_stream_header_decode(
                    &mut coder.header_flags,
                    &coder.temp[coder.temp_size..],
                ));
                if ret != LzmaRet::Ok {
                    return ret;
                }
                Sequence::HeaderCompare
            }

            Sequence::HeaderCompare => {
                let ret = lzma_stream_flags_compare(&coder.header_flags, &coder.footer_flags);
                if ret != LzmaRet::Ok {
                    return ret;
                }

                if lzma_index_stream_flags(
                    &mut *coder.this_index.as_ref().unwrap().lock().unwrap(),
                    &coder.footer_flags,
                ) != LzmaRet::Ok
                {
                    return LzmaRet::ProgError;
                }

                if lzma_index_stream_padding(
                    coder.this_index.as_ref().unwrap(),
                    coder.stream_padding,
                ) != LzmaRet::Ok
                {
                    return LzmaRet::ProgError;
                }

                coder.stream_padding = 0;

                if let Some(combined_index) = coder.combined_index.as_mut() {
                    let ret = {
                        let mut this_lock = coder.this_index.as_mut().unwrap().lock().unwrap();
                        let mut combined_lock = combined_index.lock().unwrap();
                        lzma_index_cat(&mut *this_lock, &mut *combined_lock)
                    };
                    if ret != LzmaRet::Ok {
                        return ret;
                    }
                }

                coder.combined_index = Some(coder.this_index.take().unwrap());

                if coder.file_target_pos == 0 {
                    assert!(
                        lzma_index_file_size(coder.combined_index.as_ref().unwrap().clone())
                            == coder.file_size
                    );

                    coder.dest_index = coder
                        .combined_index
                        .take()
                        .map(|index| Arc::new(Mutex::new(index)));

                    *in_pos = in_size;
                    return LzmaRet::StreamEnd;
                }

                if coder.temp_size > 0 {
                    Sequence::PaddingDecode
                } else {
                    Sequence::PaddingSeek
                }
            }

            _ => {
                debug_assert!(false);
                return LzmaRet::ProgError;
            }
        };
        coder.sequence = next_sequence;
    }
}

fn file_info_decoder_memconfig(
    coder_ptr: &mut CoderType,
    memusage: &mut u64,
    old_memlimit: &mut u64,
    new_memlimit: u64,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_mut::<LzmaFileInfoCoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::FileInfo(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // (1) 获取已经解码并处理到 combined_index 中的索引的内存使用情况
    let mut combined_index_memusage = 0;
    let mut this_index_memusage = 0;

    if let Some(combined_index) = &coder.combined_index {
        combined_index_memusage = lzma_index_memused(combined_index.clone());
    }

    // (2) 或 (3) 或都没有：选择处理的索引
    if let Some(this_index) = &coder.this_index {
        // (2) 如果最新的索引已经可用，使用其内存使用情况
        this_index_memusage =
            lzma_index_memused(Arc::new(Mutex::new(this_index.lock().unwrap().clone())));
    } else if coder.sequence == Sequence::IndexDecode {
        // (3) 如果索引解码器已激活且尚未将新索引存储到 this_index 中
        let mut dummy = 0;
        if coder.index_decoder.memconfig.unwrap()(
            &mut coder.index_decoder.coder.as_mut().unwrap(),
            &mut this_index_memusage,
            &mut dummy,
            0,
        ) != LzmaRet::Ok
        {
            assert!(false);
            return LzmaRet::ProgError;
        }
    }

    // 计算总的内存使用量
    *memusage = combined_index_memusage + this_index_memusage;
    if *memusage == 0 {
        *memusage = lzma_index_memusage(1, 0);
    }

    *old_memlimit = coder.memlimit;

    // 如果请求了新的内存使用限制，设置新的内存使用限制
    if new_memlimit != 0 {
        if new_memlimit < *memusage {
            return LzmaRet::MemlimitError;
        }

        // 在条件 (3) 中，告诉索引解码器新的内存使用限制
        if coder.this_index.is_none() && coder.sequence == Sequence::IndexDecode {
            let idec_new_memlimit = new_memlimit - combined_index_memusage;

            assert!(this_index_memusage > 0);
            assert!(idec_new_memlimit > 0);

            let mut dummy1 = 0;
            let mut dummy2 = 0;

            if coder.index_decoder.memconfig.unwrap()(
                &mut coder.index_decoder.coder.as_mut().unwrap(),
                &mut dummy1,
                &mut dummy2,
                idec_new_memlimit,
            ) != LzmaRet::Ok
            {
                assert!(false);
                return LzmaRet::ProgError;
            }
        }

        coder.memlimit = new_memlimit;
    }

    LzmaRet::Ok
}

fn file_info_decoder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::FileInfo(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };

    lzma_next_end(&mut coder.index_decoder);
    // lzma_index_end(coder.this_index, allocator);
    // lzma_index_end(coder.combined_index, allocator);
    // 使用 take() 方法安全地取出 this_index 和 combined_index
    if let Some(index_arc) = coder.this_index.take() {
        let mut index = index_arc.lock().unwrap();
        lzma_index_end(&mut *index);
    }

    if let Some(index_arc) = coder.combined_index.take() {
        let mut index = index_arc.lock().unwrap();
        lzma_index_end(&mut *index);
    }
}

fn lzma_file_info_decoder_init(
    next: &mut LzmaNextCoder,
    seek_pos: &mut u64,
    dest_index: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    memlimit: u64,
    file_size: u64,
) -> LzmaRet {
    // lzma_next_coder_init!(&lzma_file_info_decoder_init,next, allocator);
    if next.init
        != Some(NextCoderInitFunction::FileInfoDecoder(
            lzma_file_info_decoder_init,
        ))
    {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::FileInfoDecoder(
        lzma_file_info_decoder_init,
    ));

    // if dest_index.is_none() {    // 因为 dest_index 是 Option<Box<Box<LzmaIndex>>> Box类型的值不可能为空，所以不用判断了
    //     return LzmaRet::ProgError;
    // }

    let mut coder = &mut LzmaFileInfoCoder::default();

    if next.coder.is_none() {
        let mut coder_ = LzmaFileInfoCoder::default();
        next.end = Some(file_info_decoder_end);
        next.memconfig = Some(file_info_decoder_memconfig);
        next.code = Some(file_info_decode);

        coder_.index_decoder = Box::new(LzmaNextCoder::default());
        coder_.this_index = None;
        coder_.combined_index = None;
        next.coder = Some(CoderType::FileInfo(coder_));
    }
    coder = match next.coder {
        Some(CoderType::FileInfo(ref mut c)) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.sequence = Sequence::MagicBytes;
    coder.file_cur_pos = 0;
    coder.file_target_pos = 0;
    coder.file_size = file_size;

    if let Some(index_arc) = coder.this_index.take() {
        let mut index = index_arc.lock().unwrap();
        lzma_index_end(&mut *index);
    }

    if let Some(index_arc) = coder.combined_index.take() {
        let mut index = index_arc.lock().unwrap();
        lzma_index_end(&mut *index);
    }

    coder.stream_padding = 0;
    coder.dest_index = dest_index.clone();
    coder.external_seek_pos = Some(*seek_pos);

    coder.memlimit = my_max(1, memlimit);

    coder.temp_pos = 0;
    coder.temp_size = LZMA_STREAM_HEADER_SIZE;

    LzmaRet::Ok
}

pub fn get_dest_index(strm: &LzmaStream) -> Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>> {
    if strm.internal.borrow().is_some() {
        let mut internal = strm.internal.borrow_mut();
        let next = internal.as_mut().unwrap().next.as_mut().unwrap();
        let coder = match next.coder {
            Some(CoderType::FileInfo(ref mut c)) => c,
            _ => return None,
        };
        coder.dest_index.clone()
    } else {
        None
    }
}

impl LzmaFileInfoCoder {
    pub(crate) fn external_seek_pos(&self) -> Option<u64> {
        self.external_seek_pos
    }
}

pub fn lzma_file_info_decoder(
    strm: &mut LzmaStream,
    dest_index: Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    memlimit: u64,
    file_size: u64,
) -> LzmaRet {
    let ret_: LzmaRet = lzma_strm_init(Some(strm));
    if ret_ != LzmaRet::Ok {
        return ret_;
    }
    let mut seek_pos = strm.seek_pos.get();
    let ret_0: LzmaRet = lzma_file_info_decoder_init(
        &mut strm
            .internal
            .borrow_mut()
            .as_mut()
            .unwrap()
            .next
            .as_mut()
            .unwrap(),
        &mut seek_pos,
        dest_index,
        memlimit,
        file_size,
    );
    strm.seek_pos.set(seek_pos);
    if ret_0 != LzmaRet::Ok {
        lzma_end(Some(strm));
        return ret_0;
    }

    strm.internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .supported_actions[LzmaAction::Run as usize] = true;
    strm.internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .supported_actions[LzmaAction::Finish as usize] = true;

    LzmaRet::Ok
}
