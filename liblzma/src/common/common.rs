/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::LzmaAction;
use crate::api::LzmaCheck;
use crate::api::LzmaFilter;
use crate::api::LzmaRet;
use crate::api::LzmaVli;

pub const LZMA_ACTION_MAX: usize = LzmaAction::FullBarrier as usize;
pub const LZMA_BUFFER_SIZE: usize = 4096;
pub const LZMA_THREADS_MAX: u32 = 16384;

// 用于存储与 lzma_strm_init、lzma_code 和 lzma_end 函数相关的内部数据。
// 这个结构体包含了关于编码器（或解码器）操作的各种信息，
// 用于验证、跟踪和控制编码/解码过程中发生的状态和操作
#[derive(Debug)]
pub struct LzmaInternal {
    pub next: Option<Box<LzmaNextCoder>>,
    pub sequence: Sequence,
    // 这是 lzma_stream 中 avail_in 的副本，表示输入流中可用的字节数。
    pub avail_in: usize,
    pub supported_actions: [bool; LZMA_ACTION_MAX + 1],
    pub allow_buf_error: bool,
}

// 追踪编码器的当前操作状态
#[derive(PartialEq, Debug)]
pub enum Sequence {
    Run,
    SyncFlush,
    FullFlush,
    Finish,
    FullBarrier,
    End,
    Error,
}

#[derive(Debug)]
pub enum CoderType {
    // AloneDecoder(LzmaAloneDecoder),
    // AloneEncoder(LzmaAloneEncoder),
    // AutoDecoder(LzmaAutoCoder),
    // BlockDecoder(LzmaBlockDecoder),
    // BlockEncoder(LzmaBlockEncoder),
    // FileInfo(LzmaFileInfoCoder),
    // IndexDecoder(LzmaIndexDecoder),
    // IndexEncoder(LzmaIndexEncoder),
    // LzipDecoder(LzmaLzipCoder),
    // MicroLzamDecoder(LzmaMicrolzmaDecoder),
    // MicroLzamEncoder(LzmaMicrolzmaEncoder),
    // StreamDecoder(LzmaStreamDecoder),
    // StreamEncoder(LzmaStreamEncoder),
    // DeltaCoder(LzmaDeltaCoder),
    // LzDecoder(LzmaDecoder),
    // LzEncoder(LzmaEncoder),
    // SimpleCoder(LzmaSimpleCoder),
    // StreamEncoderMt(LzmaStreamEncoderMt<'a>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum NextCoderInitFunction {
    // AloneDecoder(fn(&mut LzmaNextCoder, u64, bool) -> LzmaRet),
    // AutoDecoder(fn(&mut LzmaNextCoder, u64, u32) -> LzmaRet),
    // RawDecoder(fn(&mut LzmaNextCoder, &[LzmaFilter]) -> LzmaRet),
    // IndexDecoder(fn(&mut LzmaNextCoder, Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>, u64) -> LzmaRet),
    // FilterInfo(fn(&mut LzmaNextCoder, &[LzmaFilterInfo]) -> LzmaRet),
    // AloneEncoder(fn(&mut LzmaNextCoder, &LzmaOptionsLzma) -> LzmaRet),
    // BlockDecoder(fn(&mut LzmaNextCoder, &mut LzmaBlock) -> LzmaRet),
    // BlockEncoder(fn(&mut LzmaNextCoder, &LzmaBlock) -> LzmaRet),
    // FileInfoDecoder(
    //     fn(
    //         &mut LzmaNextCoder,
    //         &mut u64,
    //         Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
    //         u64,
    //         u64,
    //     ) -> LzmaRet,
    // ),
    // IndexEncoder(fn(&mut LzmaNextCoder, &Box<LzmaIndex>) -> LzmaRet),
    // LzipDecoder(fn(&mut LzmaNextCoder, u64, u32) -> LzmaRet),
    // MicroLzamDecoder(fn(&mut LzmaNextCoder, u64, u64, bool, u32) -> LzmaRet),
    // MicroLzamEncoder(fn(&mut LzmaNextCoder, &LzmaOptionsLzma) -> LzmaRet),
    // StreamDecoder(fn(&mut LzmaNextCoder, u64, u32) -> LzmaRet),
    // StreamEncoder(fn(&mut LzmaNextCoder, Option<&[LzmaFilter]>, LzmaCheck) -> LzmaRet),
}

pub type LzmaCodeFunction = fn(
    coder: &mut CoderType,
    in_: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet;

#[repr(C)]
#[derive(Debug)]
pub struct LzmaNextCoder {
    // 指向编码器或解码器的指针。
    pub coder: Option<CoderType>,
    // 编码器或解码器的 ID。
    pub id: LzmaVli,
    /// "Pointer" to init function. This is never called here.
    /// We need only to detect if we are initializing a coder
    /// that was allocated earlier. See lzma_next_coder_init and
    /// lzma_next_strm_init macros in this file.
    pub init: Option<NextCoderInitFunction>,
    // 指向执行实际编码操作的函数的指针。
    pub code: Option<LzmaCodeFunction>,
    // pub code: Option<codeType>,
    // 指向结束函数的指针，用于释放编码器相关的资源。
    pub end: Option<fn(coder: &mut CoderType)>,
    // 指向获取进度信息的函数指针。
    pub get_progress:
        Option<fn(coder: &mut CoderType, progress_in: &mut u64, progress_out: &mut u64)>,
    // 指向返回完整性校验类型的函数指针。
    pub get_check: Option<fn(coder: &mut CoderType) -> LzmaCheck>,
    // 指向设置或获取内存配置的函数指针。
    pub memconfig: Option<
        fn(
            coder: &mut CoderType,
            memusage: &mut u64,
            old_memlimit: &mut u64,
            new_memlimit: u64,
        ) -> LzmaRet,
    >,
    // 指向更新编码器选项的函数指针。
    pub update: Option<
        fn(
            coder: &mut CoderType,

            filters: Option<&[LzmaFilter]>,
            reversed_filters: &[LzmaFilter],
        ) -> LzmaRet,
    >,
    // 指向设置输出限制的函数指针。
    pub set_out_limit:
        Option<fn(coder: &mut CoderType, uncomp_size: &mut u64, out_limit: u64) -> LzmaRet>,
}
