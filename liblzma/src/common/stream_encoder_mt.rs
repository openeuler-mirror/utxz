/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::api::{
    LzmaAction, LzmaAllocator, LzmaBlock, LzmaCheck, LzmaFilter, LzmaRet, LzmaStream,
    LzmaStreamFlags, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_UNKNOWN,
};
use crate::check::{lzma_check_is_supported, lzma_check_size, lzma_crc32};

use super::{
    lzma_block_buffer_bound, lzma_block_buffer_encode, lzma_block_unpadded_size, lzma_cputhreads,
    lzma_end, lzma_mt_block_size, lzma_stream_footer_encode, lzma_stream_header_encode,
    lzma_strm_init, lzma_vli_encode, CoderType, LzmaNextCoder, LZMA_THREADS_MAX,
};

const INDEX_INDICATOR: u8 = 0x00;

/// Block record stored for index encoding: (unpadded_size, uncompressed_size)
#[derive(Debug)]
struct BlockRecord {
    unpadded_size: u64,
    uncompressed_size: u64,
}

fn encode_index(
    records: &[(u64, u64)],
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> LzmaRet {
    // Save index start position for CRC calculation
    let index_start = *out_pos;

    // Index Indicator
    if out_size - *out_pos < 1 {
        return LzmaRet::BufError;
    }
    output[*out_pos] = INDEX_INDICATOR;
    *out_pos += 1;

    // Number of Records (VLI encoded)
    let ret = lzma_vli_encode(records.len() as u64, None, output, out_pos, out_size);
    if ret != LzmaRet::Ok {
        return ret;
    }

    // Each record: unpadded_size + uncompressed_size (both VLI encoded)
    for &(unpadded_size, uncompressed_size) in records {
        let ret = lzma_vli_encode(unpadded_size, None, output, out_pos, out_size);
        if ret != LzmaRet::Ok {
            return ret;
        }
        let ret = lzma_vli_encode(uncompressed_size, None, output, out_pos, out_size);
        if ret != LzmaRet::Ok {
            return ret;
        }
    }

    // Padding to 4-byte boundary
    while *out_pos % 4 != 0 {
        if *out_pos >= out_size {
            return LzmaRet::BufError;
        }
        output[*out_pos] = 0;
        *out_pos += 1;
    }

    // CRC32 of the index data (from indicator through padding)
    let crc = lzma_crc32(&output[index_start..*out_pos], *out_pos - index_start, 0);
    if out_size - *out_pos < 4 {
        return LzmaRet::BufError;
    }
    output[*out_pos..*out_pos + 4].copy_from_slice(&crc.to_le_bytes());
    *out_pos += 4;

    LzmaRet::Ok
}

/// Calculate index size for footer's backward_size field.
/// Format: 1 (indicator) + vli(record_count) + records * (2 * vli_bytes) + padding + 4 (crc)
/// This is an estimate — we compute it precisely after encoding.
fn index_size_estimate(records: &[BlockRecord]) -> u64 {
    // indicator (1) + record_count VLI (max 9) + each record (2 * max 9) + padding (3) + crc (4)
    let mut size: u64 = 1; // indicator
                           // Use a temporary buffer to measure exact VLI sizes
    let mut tmp = [0u8; 16];
    let mut pos = 0;
    let _ = lzma_vli_encode(records.len() as u64, None, &mut tmp, &mut pos, 16);
    size += pos as u64;

    for rec in records {
        pos = 0;
        let _ = lzma_vli_encode(rec.unpadded_size, None, &mut tmp, &mut pos, 16);
        size += pos as u64;
        pos = 0;
        let _ = lzma_vli_encode(rec.uncompressed_size, None, &mut tmp, &mut pos, 16);
        size += pos as u64;
    }

    // Pad to 4 bytes
    size = (size + 3) & !3;
    size += 4; // CRC32
    size
}

struct InputBlock {
    seq: usize,
    data: Vec<u8>,
    is_last: bool,
}

#[derive(Debug)]
struct CompressedBlock {
    seq: usize,
    data: Vec<u8>,
    unpadded_size: u64,
    uncompressed_size: u64,
    is_last: bool,
}

fn worker_thread(
    rx: Arc<Mutex<Receiver<InputBlock>>>,
    filters: Vec<LzmaFilter>,
    check: LzmaCheck,
    outbuf_size: usize,
    tx: Sender<CompressedBlock>,
) {
    let allocator = LzmaAllocator::default();

    loop {
        let mut input = {
            let guard = rx.lock().unwrap();
            match guard.recv() {
                Ok(block) => block,
                Err(_) => return,
            }
        };
        let mut block = LzmaBlock {
            version: 0,
            check: check.clone(),
            filters: filters.clone(),
            compressed_size: LZMA_VLI_UNKNOWN,
            ..Default::default()
        };

        let mut output = vec![0u8; outbuf_size];
        let mut out_pos = 0;
        let input_len = input.data.len();

        let ret = lzma_block_buffer_encode(
            &mut block,
            &allocator,
            &mut input.data,
            input_len,
            &mut output,
            &mut out_pos,
            outbuf_size,
        );

        if ret != LzmaRet::Ok {
            tx.send(CompressedBlock {
                seq: input.seq,
                data: Vec::new(),
                unpadded_size: 0,
                uncompressed_size: 0,
                is_last: input.is_last,
            })
            .ok();
            return;
        }

        output.truncate(out_pos);

        // Use lzma_block_unpadded_size for correct unpadded_size calculation.
        let unpadded_size = lzma_block_unpadded_size(&block);

        // block_buffer_encode sets block.uncompressed_size to
        // LZMA_VLI_UNKNOWN for block header encoding, so we use
        // the original input length instead.
        let uncompressed_size = input_len as u64;

        tx.send(CompressedBlock {
            seq: input.seq,
            data: output,
            unpadded_size,
            uncompressed_size,
            is_last: input.is_last,
        })
        .ok();
    }
}

pub fn lzma_stream_encoder_mt(
    strm: &mut LzmaStream,
    filters: &[LzmaFilter],
    check: LzmaCheck,
    threads: u32,
) -> LzmaRet {
    if !lzma_check_is_supported(check.clone()) {
        return LzmaRet::UnsupportedCheck;
    }

    if threads < 1 || threads > LZMA_THREADS_MAX {
        return LzmaRet::OptionsError;
    }

    let block_size = lzma_mt_block_size(filters);
    if block_size == 0 {
        return LzmaRet::OptionsError;
    }

    let ret = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    let outbuf_size = lzma_block_buffer_bound(block_size as usize);
    if outbuf_size == 0 {
        lzma_end(Some(strm));
        return LzmaRet::MemError;
    }

    let filters_vec: Vec<LzmaFilter> = filters.to_vec();
    let check_val: LzmaCheck = check.clone();

    let mut internal = strm.internal.borrow_mut();
    let internal = internal.as_mut().unwrap();

    let mt_coder = MtStreamEncoder::new(
        filters_vec,
        check_val,
        threads,
        block_size as usize,
        outbuf_size,
    );

    internal.next = Some(Box::new(LzmaNextCoder {
        coder: Some(CoderType::MtStreamEncoder(mt_coder)),
        code: Some(mt_encode_code),
        ..Default::default()
    }));

    let supported = &mut internal.supported_actions;
    supported[LzmaAction::Run as usize] = true;
    supported[LzmaAction::Finish as usize] = true;
    supported[LzmaAction::FullFlush as usize] = true;
    supported[LzmaAction::SyncFlush as usize] = true;

    LzmaRet::Ok
}

#[derive(Debug)]
pub struct MtStreamEncoder {
    sequence: MtSequence,
    filters: Vec<LzmaFilter>,
    check: LzmaCheck,
    threads: u32,
    block_size: usize,
    outbuf_size: usize,

    input_tx: Option<Sender<InputBlock>>,
    workers: Vec<JoinHandle<()>>,
    output_rx: Option<Receiver<CompressedBlock>>,

    pending: Vec<CompressedBlock>,
    next_seq: usize,

    block_buf: Vec<u8>,
    block_seq: usize,

    header: [u8; LZMA_STREAM_HEADER_SIZE],
    header_pos: usize,

    /// Block records for index encoding: (unpadded_size, uncompressed_size)
    block_records: Vec<BlockRecord>,

    stream_flags: LzmaStreamFlags,

    output_buf: Vec<u8>,
    output_pos: usize,

    total_in: u64,
    total_out: u64,

    finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
enum MtSequence {
    StreamHeader,
    Block,
    Index,
    StreamFooter,
    End,
}

impl MtStreamEncoder {
    fn new(
        filters: Vec<LzmaFilter>,
        check: LzmaCheck,
        threads: u32,
        block_size: usize,
        outbuf_size: usize,
    ) -> Self {
        let stream_flags = LzmaStreamFlags {
            version: 0,
            check,
            ..Default::default()
        };

        let mut header = [0u8; LZMA_STREAM_HEADER_SIZE];
        let _ = lzma_stream_header_encode(&stream_flags, &mut header);

        MtStreamEncoder {
            sequence: MtSequence::StreamHeader,
            filters,
            check,
            threads,
            block_size,
            outbuf_size,
            input_tx: None,
            workers: Vec::new(),
            output_rx: None,
            pending: Vec::new(),
            next_seq: 0,
            block_buf: Vec::with_capacity(block_size),
            block_seq: 0,
            header,
            header_pos: 0,
            block_records: Vec::new(),
            stream_flags,
            output_buf: Vec::new(),
            output_pos: 0,
            total_in: 0,
            total_out: 0,
            finished: false,
        }
    }

    fn start_workers(&mut self) {
        if self.input_tx.is_some() {
            return;
        }

        let (input_tx, worker_rx) = mpsc::channel::<InputBlock>();
        let (output_tx, output_rx) = mpsc::channel::<CompressedBlock>();
        let worker_rx = Arc::new(Mutex::new(worker_rx));

        for _ in 0..self.threads {
            let rx = Arc::clone(&worker_rx);
            let tx = output_tx.clone();
            let filters = self.filters.clone();
            let check = self.check.clone();
            let outbuf_size = self.outbuf_size;

            let handle = std::thread::Builder::new()
                .stack_size(8 * 1024 * 1024)
                .spawn(move || {
                    worker_thread(rx, filters, check, outbuf_size, tx);
                })
                .expect("Failed to spawn worker thread");
            self.workers.push(handle);
        }

        drop(output_tx);

        self.input_tx = Some(input_tx);
        self.output_rx = Some(output_rx);
    }

    fn collect_results(&mut self) {
        if let Some(rx) = &self.output_rx {
            while let Ok(block) = rx.try_recv() {
                self.pending.push(block);
            }
        }
    }

    fn try_dispatch_block(&mut self, is_last: bool) {
        if self.block_buf.is_empty() && !is_last {
            return;
        }

        if !is_last && self.block_buf.len() < self.block_size {
            return;
        }

        let data = std::mem::replace(&mut self.block_buf, Vec::with_capacity(self.block_size));
        self.total_in += data.len() as u64;

        let input_block = InputBlock {
            seq: self.block_seq,
            data,
            is_last,
        };
        self.block_seq += 1;

        if let Some(tx) = &self.input_tx {
            let _ = tx.send(input_block);
        }
    }

    fn record_block(&mut self, block: &CompressedBlock) {
        self.block_records.push(BlockRecord {
            unpadded_size: block.unpadded_size,
            uncompressed_size: block.uncompressed_size,
        });
    }

    fn get_next_output(&mut self) -> Option<Vec<u8>> {
        self.pending.sort_by_key(|b| b.seq);

        while let Some(block) = self.pending.first() {
            if block.seq == self.next_seq {
                let block = self.pending.remove(0);
                self.next_seq += 1;

                if block.data.is_empty() && !block.is_last {
                    self.finished = true;
                    return None;
                }

                // Record all blocks for the Index
                self.record_block(&block);
                return Some(block.data);
            } else {
                break;
            }
        }
        None
    }

    fn finalize(&mut self) {
        // Drop input sender so workers know no more work is coming
        self.input_tx.take();

        // Collect all remaining results from workers
        if let Some(rx) = &self.output_rx {
            while let Ok(block) = rx.try_recv() {
                self.pending.push(block);
            }
        }

        // Join all worker threads (take the receiver first to avoid blocking)
        if let Some(rx) = self.output_rx.take() {
            // Block until all workers are done
            while let Ok(block) = rx.recv() {
                self.pending.push(block);
            }
        }

        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }

        // Sort pending blocks by sequence number
        self.pending.sort_by_key(|b| b.seq);

        // Calculate index size for the footer
        self.stream_flags.backward_size = index_size_estimate(&self.block_records);
    }
}

fn mt_encode_code(
    coder: &mut CoderType,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    let mt = match coder {
        CoderType::MtStreamEncoder(ref mut m) => m,
        _ => return LzmaRet::ProgError,
    };

    // Phase 1: Write stream header
    if mt.sequence == MtSequence::StreamHeader {
        let copy_len = (mt.header.len() - mt.header_pos).min(out_size - *out_pos);
        output[*out_pos..*out_pos + copy_len]
            .copy_from_slice(&mt.header[mt.header_pos..mt.header_pos + copy_len]);
        *out_pos += copy_len;
        mt.header_pos += copy_len;
        mt.total_out += copy_len as u64;

        if mt.header_pos < mt.header.len() {
            return LzmaRet::Ok;
        }
        mt.header_pos = 0;
        mt.sequence = MtSequence::Block;
    }

    // Phase 2: Encode blocks using worker threads
    if mt.sequence == MtSequence::Block {
        mt.start_workers();

        // Feed input into block buffer and dispatch to workers
        while *in_pos < in_size {
            let remaining = in_size - *in_pos;
            let space = mt.block_size - mt.block_buf.len();
            let to_copy = remaining.min(space);

            mt.block_buf
                .extend_from_slice(&input[*in_pos..*in_pos + to_copy]);
            *in_pos += to_copy;

            mt.try_dispatch_block(false);
        }

        if action == LzmaAction::Finish && !mt.finished {
            mt.try_dispatch_block(true);
            mt.finalize();
            mt.finished = true;
        } else if action == LzmaAction::FullFlush || action == LzmaAction::SyncFlush {
            mt.try_dispatch_block(true);
        }

        // Collect and write available output
        mt.collect_results();
        while *out_pos < out_size {
            if !mt.output_buf.is_empty() {
                break;
            }
            if let Some(data) = mt.get_next_output() {
                let copy_len = data.len().min(out_size - *out_pos);
                output[*out_pos..*out_pos + copy_len].copy_from_slice(&data[..copy_len]);
                *out_pos += copy_len;
                mt.total_out += copy_len as u64;

                if copy_len < data.len() {
                    mt.output_buf = data[copy_len..].to_vec();
                    mt.output_pos = 0;
                    break;
                }
            } else {
                break;
            }
        }

        // Write buffered remainder
        if mt.sequence == MtSequence::Block && !mt.output_buf.is_empty() {
            let copy_len = (mt.output_buf.len() - mt.output_pos).min(out_size - *out_pos);
            output[*out_pos..*out_pos + copy_len]
                .copy_from_slice(&mt.output_buf[mt.output_pos..mt.output_pos + copy_len]);
            *out_pos += copy_len;
            mt.output_pos += copy_len;
            mt.total_out += copy_len as u64;

            if mt.output_pos >= mt.output_buf.len() {
                mt.output_buf.clear();
                mt.output_pos = 0;
            }
        }

        if mt.sequence == MtSequence::Block {
            if mt.finished {
                if mt.pending.is_empty() && mt.output_buf.is_empty() {
                    // Transition to Index phase. Return Ok so that the output
                    // buffer gets flushed by coder_normal before Index encoding.
                    mt.stream_flags.backward_size = index_size_estimate(&mt.block_records);
                    mt.sequence = MtSequence::Index;
                    return LzmaRet::Ok;
                } else {
                    return LzmaRet::Ok;
                }
            } else if *in_pos < in_size || action == LzmaAction::Run {
                return LzmaRet::Ok;
            }
        }
    }

    // Phase 3: Encode index
    if mt.sequence == MtSequence::Index {
        let records: Vec<(u64, u64)> = mt
            .block_records
            .iter()
            .map(|r| (r.unpadded_size, r.uncompressed_size))
            .collect();

        // Measure index start position for CRC calculation
        let index_start = *out_pos;

        let ret = encode_index(&records, output, out_pos, out_size);
        if ret != LzmaRet::Ok {
            return ret;
        }

        // Update backward_size with actual index size
        mt.stream_flags.backward_size = (*out_pos - index_start) as u64;

        mt.total_out += LZMA_STREAM_HEADER_SIZE as u64;
        mt.sequence = MtSequence::StreamFooter;
        // Return Ok so that the output buffer gets flushed before StreamFooter
        return LzmaRet::Ok;
    }

    // Phase 4: Write stream footer
    if mt.sequence == MtSequence::StreamFooter {
        let footer_len = LZMA_STREAM_HEADER_SIZE;
        if out_size - *out_pos < footer_len {
            return LzmaRet::BufError;
        }

        let _ = lzma_stream_footer_encode(&mut mt.stream_flags, &mut output[*out_pos..]);
        *out_pos += footer_len;
        mt.sequence = MtSequence::End;
        return LzmaRet::StreamEnd;
    }

    LzmaRet::Ok
}
