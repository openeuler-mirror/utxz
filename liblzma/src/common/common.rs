/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::{
    alloc::alloc,
    any::{Any, TypeId},
    sync::{Arc, Mutex},
};

use crate::common::LzmaIndex;
use crate::{
    api::{
        lzma_version_string_c, LzmaAction, LzmaBlock, LzmaCheck, LzmaFilter, LzmaOptionsLzma,
        LzmaOptionsType, LzmaReservedEnum, LzmaRet, LzmaStream, LzmaVli, LZMA_CONCATENATED,
        LZMA_FAIL_FAST, LZMA_IGNORE_CHECK, LZMA_TELL_ANY_CHECK, LZMA_TELL_NO_CHECK,
        LZMA_TELL_UNSUPPORTED_CHECK, LZMA_VERSION, LZMA_VERSION_COMMIT, LZMA_VERSION_MAJOR,
        LZMA_VERSION_MINOR, LZMA_VERSION_PATCH, LZMA_VERSION_STABILITY_STRING, LZMA_VLI_UNKNOWN,
    },
    delta::LzmaDeltaCoder,
    lz::{LzmaDecoder, LzmaEncoder},
    lzma::LzmaLzma2Decoder,
    simple::LzmaSimpleCoder,
};
use common::{memzero, my_max};

use super::{
    LzmaAloneDecoder, LzmaAloneEncoder, LzmaAutoCoder, LzmaBlockDecoder, LzmaBlockEncoder,
    LzmaFileInfoCoder, LzmaIndexDecoder, LzmaIndexEncoder, LzmaLzipCoder, LzmaMicrolzmaDecoder,
    LzmaMicrolzmaEncoder, LzmaStreamDecoder, LzmaStreamEncoder,
};

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

impl LzmaInternal {
    pub fn new() -> Self {
        Self {
            next: None,
            sequence: Sequence::Run,
            avail_in: 0,
            supported_actions: [false; LZMA_ACTION_MAX + 1],
            allow_buf_error: false,
        }
    }
}

// pub type LzmaInternal<T> = LzmaInternal<T>;

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

// type LzmaEndFunction<T> = fn(coder: Option<T>);

/// 过滤器保留 ID 的起始值
pub const LZMA_FILTER_RESERVED_START: u64 = 1 << 62;
pub const LZMA_MEMUSAGE_BASE: u64 = 1 << 15;

#[derive(Debug)]
pub enum CoderType {
    AloneDecoder(LzmaAloneDecoder),
    AloneEncoder(LzmaAloneEncoder),
    AutoDecoder(LzmaAutoCoder),
    BlockDecoder(LzmaBlockDecoder),
    BlockEncoder(LzmaBlockEncoder),
    FileInfo(LzmaFileInfoCoder),
    IndexDecoder(LzmaIndexDecoder),
    IndexEncoder(LzmaIndexEncoder),
    LzipDecoder(LzmaLzipCoder),
    MicroLzamDecoder(LzmaMicrolzmaDecoder),
    MicroLzamEncoder(LzmaMicrolzmaEncoder),
    StreamDecoder(LzmaStreamDecoder),
    StreamEncoder(LzmaStreamEncoder),
    DeltaCoder(LzmaDeltaCoder),
    LzDecoder(LzmaDecoder),
    LzEncoder(LzmaEncoder),
    SimpleCoder(LzmaSimpleCoder),
    // StreamEncoderMt(LzmaStreamEncoderMt<'a>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum NextCoderInitFunction {
    AloneDecoder(fn(&mut LzmaNextCoder, u64, bool) -> LzmaRet),
    AutoDecoder(fn(&mut LzmaNextCoder, u64, u32) -> LzmaRet),
    RawDecoder(fn(&mut LzmaNextCoder, &[LzmaFilter]) -> LzmaRet),
    IndexDecoder(fn(&mut LzmaNextCoder, Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>, u64) -> LzmaRet),
    FilterInfo(fn(&mut LzmaNextCoder, &[LzmaFilterInfo]) -> LzmaRet),
    AloneEncoder(fn(&mut LzmaNextCoder, &LzmaOptionsLzma) -> LzmaRet),
    BlockDecoder(fn(&mut LzmaNextCoder, &mut LzmaBlock) -> LzmaRet),
    BlockEncoder(fn(&mut LzmaNextCoder, &LzmaBlock) -> LzmaRet),
    FileInfoDecoder(
        fn(
            &mut LzmaNextCoder,
            &mut u64,
            Option<Arc<Mutex<Arc<Mutex<LzmaIndex>>>>>,
            u64,
            u64,
        ) -> LzmaRet,
    ),
    IndexEncoder(fn(&mut LzmaNextCoder, &Box<LzmaIndex>) -> LzmaRet),
    LzipDecoder(fn(&mut LzmaNextCoder, u64, u32) -> LzmaRet),
    MicroLzamDecoder(fn(&mut LzmaNextCoder, u64, u64, bool, u32) -> LzmaRet),
    MicroLzamEncoder(fn(&mut LzmaNextCoder, &LzmaOptionsLzma) -> LzmaRet),
    StreamDecoder(fn(&mut LzmaNextCoder, u64, u32) -> LzmaRet),
    StreamEncoder(fn(&mut LzmaNextCoder, Option<&[LzmaFilter]>, LzmaCheck) -> LzmaRet),
}

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

// impl<'a> Clone for LzmaNextCoder<'a> {
//     fn clone(&self) -> Self {
//         LzmaNextCoder {
//             coder: self.coder.clone(), // 假设 CoderType 实现了 Clone
//             id: self.id.clone(), // 假设 LzmaVli 实现了 Clone
//             init: self.init,
//             code: self.code, // 函数指针可以直接复制
//             end: self.end,
//             get_progress: self.get_progress,
//             get_check: self.get_check,
//             memconfig: self.memconfig,
//             update: self.update,
//             set_out_limit: self.set_out_limit,
//         }
//     }
// }

// pub type LzmaNextCoder<T> = LzmaNextCoderS<T>;
pub fn lzma_next_coder_init() -> LzmaNextCoder {
    LzmaNextCoder {
        coder: None,
        init: None,
        id: LZMA_VLI_UNKNOWN,
        code: None,
        end: None,
        get_progress: None,
        get_check: None,
        memconfig: None,
        update: None,
        set_out_limit: None,
    }
}
impl Default for LzmaNextCoder {
    fn default() -> Self {
        LzmaNextCoder {
            coder: None,
            init: None,
            id: LZMA_VLI_UNKNOWN,
            code: None,
            end: None,
            get_progress: None,
            get_check: None,
            memconfig: None,
            update: None,
            set_out_limit: None,
        }
    }
}

// pub const LZMA_MEMUSAGE_BASE: u64 = 1 << 15;

pub type LzmaInitFunction = fn(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet;

#[derive(Debug, Clone)]
pub struct LzmaFilterInfo {
    pub id: LzmaVli,
    pub init: Option<LzmaInitFunction>,
    pub options: Option<LzmaOptionsType>,
}
impl Default for LzmaFilterInfo {
    fn default() -> Self {
        LzmaFilterInfo {
            id: LzmaVli::default(), // 假设 LzmaVli 实现了 Default
            init: None,             // Option 类型默认是 None
            options: None,          // Option 类型默认是 None
        }
    }
}

#[macro_export]
macro_rules! lzma_next_coder_init {
    ($func:expr, $next:expr,  ) => {
        if $next.init != Some($func) {
            lzma_next_end($next);
        }
        $next.init = Some($func);
    };
}

/////////////
// Version //
/////////////
pub fn lzma_version_number() -> u32 {
    LZMA_VERSION
}

pub fn lzma_version_string() -> String {
    lzma_version_string_c(
        LZMA_VERSION_MAJOR,
        LZMA_VERSION_MINOR,
        LZMA_VERSION_PATCH,
        LZMA_VERSION_STABILITY_STRING,
        LZMA_VERSION_COMMIT,
    )
}

// pub fn lzma_alloc(size: usize, allocator: Option<&LzmaAllocator>) -> Box<dyn std::any::Any>{
//     let size = if size == 0 { 1 } else { size };
//     let ptr: Box<dyn std::any::Any>;

//     unsafe {
//         if !allocator.is_none() && allocator.unwrap().alloc.is_some() {
//             ptr = (allocator.unwrap().alloc.unwrap())(allocator.unwrap().opaque, 1, size);
//         } else {
//             ptr = Box::new(libc::malloc(size));
//         }
//     }

//     ptr
// }

// pub fn lzma_alloc_zero(size: usize, allocator: *const LzmaAllocator) -> Box<dyn std::any::Any> {
//     let size = if size == 0 { 1 } else { size };

//     unsafe {
//         if !allocator.is_null() && (*allocator).alloc.is_some() {
//             let ptr = ((*allocator).alloc.unwrap())((*allocator).opaque, 1, size);
//             if let Some(mut vec) = ptr.downcast_mut::<Vec<u8>>() {
//                 vec.resize(size, 0);
//                 return ptr;
//             }
//             // 如果不是 Vec<u8>，则直接返回原始指针
//             return ptr;
//         }

//         // 使用 Vec<u8> 进行零初始化
//         let mut vec = Vec::with_capacity(size);
//         vec.resize(size, 0);

//         Box::new(vec) as Box<dyn std::any::Any>
//     }
// }

// pub fn lzma_free(ptr: &mut Box<dyn std::any::Any>, allocator: &mut LzmaAllocator) {
//     unsafe {
//         if !allocator.is_null() && (*allocator).free.is_some() {
//             ((*allocator).free.unwrap())((*allocator).opaque, *ptr);
//         } else {
//             libc::free(Box::into_raw(*ptr) as *mut libc::c_void);
//         }
//     }
// }

// pub fn lzma_free(ptr: &mut Option<&mut Box<dyn std::any::Any>>, allocator: Option<&LzmaAllocator>) {
//     if let Some(alloc) = allocator {
//         if let Some(free_fn) = alloc.free {
//             // 如果有自定义的 free 函数，我们调用它
//             free_fn(alloc.opaque, Box::new(ptr.take().unwrap()));
//         }
//     } else {
//         // 如果没有 allocator，直接利用 Rust 自带的内存释放机制
//         // 由于我们使用 Option<Box> 类型，它会在这里自动释放内存
//         ptr.take(); // 释放 Box 中的值
//     }
// }

//////////
// Misc //
//////////

pub fn lzma_bufcpy(
    in_0: &[u8],
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
) -> usize {
    let in_avail: usize = in_size - *in_pos;
    let out_avail: usize = out_size - *out_pos;
    let copy_size: usize = if in_avail < out_avail {
        in_avail
    } else {
        out_avail
    };

    let out_pose_t = *out_pos;
    let in_pose_t = *in_pos;

    // println!("input first 10 bytes: {:?}", &in_0[..in_0.len().min(10)]);

    if copy_size > 0 {
        out[out_pose_t..out_pose_t + copy_size]
            .copy_from_slice(&in_0[in_pose_t..in_pose_t + copy_size]);
    }

    // println!("output first 10 bytes: {:?}", &out[..out.len().min(10)]);

    *in_pos = (*in_pos).wrapping_add(copy_size);
    *out_pos = (*out_pos).wrapping_add(copy_size);
    return copy_size;
}

pub fn lzma_next_filter_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    if let Some(init_fn) = filters[0].init {
        lzma_next_coder_init!(NextCoderInitFunction::FilterInfo(init_fn), next,);
        next.id = filters[0].id;
        init_fn(next, filters)
    } else {
        LzmaRet::Ok
    }
}

pub fn lzma_next_filter_update(
    next: &mut LzmaNextCoder,

    reversed_filters: &[LzmaFilter],
) -> LzmaRet {
    if reversed_filters[0].id != next.id {
        return LzmaRet::ProgError;
    }

    if reversed_filters[0].id == LZMA_VLI_UNKNOWN {
        return LzmaRet::Ok;
    }

    assert!(next.update.is_some());
    let dummy_filter = LzmaFilter {
        id: 0,
        options: None,
    };

    let mut ret = LzmaRet::Ok;
    if let Some(update) = next.update {
        ret = update(
            next.coder.as_mut().unwrap(),
            Some(&[dummy_filter]),
            reversed_filters,
        );
    };
    return ret;
}

pub fn lzma_next_end(next: &mut LzmaNextCoder) {
    if next.init.is_some() {
        if let Some(end_fn) = next.end {
            end_fn(next.coder.as_mut().unwrap());
        }
        *next = lzma_next_coder_init();
    }
}

//////////////////////////////////////
// External to internal API wrapper //
//////////////////////////////////////
pub fn lzma_strm_init(strm: Option<&mut LzmaStream>) -> LzmaRet {
    match strm {
        Some(strm) => {
            if strm.internal.borrow_mut().as_mut().is_none() {
                // strm.internal.borrow_mut().as_mut().unwrap().next = Some(Box::new(LzmaNextCoder::default()));
                *strm.internal.borrow_mut() = Some(LzmaInternal::new());
                strm.internal.borrow_mut().as_mut().unwrap().next =
                    Some(Box::new(LzmaNextCoder::default()));
            }

            memzero(
                &mut strm
                    .internal
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .supported_actions,
            );
            strm.internal.borrow_mut().as_mut().unwrap().sequence = Sequence::Run;
            strm.internal.borrow_mut().as_mut().unwrap().allow_buf_error = false;

            strm.total_in.set(0);
            strm.total_out.set(0);

            LzmaRet::Ok
        }
        None => {
            return LzmaRet::ProgError;
        }
    }
}

// pub fn check_box_is_null(boxed: &mut Box<dyn Any>) -> bool {
//     // 将 Box 转换为原始指针
//     let raw_ptr = Box::into_raw(*boxed);

//     // 检查指针是否为 null
//     let is_null = raw_ptr.is_null();

//     // 如果不是 null，则重新创建 Box 以确保内存不会泄漏
//     // if !is_null {
//     //     unsafe { Box::from_raw(raw_ptr); }
//     // }

//     is_null
// }

pub fn lzma_code(strm: &mut LzmaStream, action: LzmaAction) -> LzmaRet {
    // Sanity checks
    let mut internal = strm.internal.borrow_mut();
    let internal = internal.as_mut().unwrap();

    if (strm.next_in.is_empty() && strm.avail_in.get() != 0)
        || (strm.next_out.borrow().is_empty() && strm.avail_out.get() != 0)
        || internal.next.as_mut().unwrap().code.is_none()
        || (action.clone() as u32) > LZMA_ACTION_MAX as u32
        || !internal.supported_actions[action.clone() as usize]
    {
        return LzmaRet::ProgError;
    }

    // Check if unsupported members have been set to non-zero or non-NULL
    if strm.reserved_int2.get() != 0
        || strm.reserved_int3.get() != 0
        || strm.reserved_int4.get() != 0
        || strm.reserved_enum1.get() != 0
        || strm.reserved_enum2.get() != 0
    {
        return LzmaRet::OptionsError;
    }

    // 处理序列状态
    match internal.sequence {
        Sequence::Run => match action {
            LzmaAction::Run => {}
            LzmaAction::SyncFlush => {
                internal.sequence = Sequence::SyncFlush;
            }
            LzmaAction::FullFlush => {
                internal.sequence = Sequence::FullFlush;
            }
            LzmaAction::Finish => {
                internal.sequence = Sequence::Finish;
            }
            LzmaAction::FullBarrier => {
                internal.sequence = Sequence::FullBarrier;
            }
        },
        Sequence::SyncFlush => {
            if action != LzmaAction::SyncFlush || internal.avail_in != strm.avail_in.get() {
                return LzmaRet::ProgError;
            }
        }
        Sequence::FullFlush => {
            if action != LzmaAction::FullFlush || internal.avail_in != strm.avail_in.get() {
                return LzmaRet::ProgError;
            }
        }
        Sequence::Finish => {
            if action != LzmaAction::Finish || internal.avail_in != strm.avail_in.get() {
                return LzmaRet::ProgError;
            }
        }
        Sequence::FullBarrier => {
            if action != LzmaAction::FullBarrier || internal.avail_in != strm.avail_in.get() {
                return LzmaRet::ProgError;
            }
        }
        Sequence::End => return LzmaRet::StreamEnd,
        Sequence::Error => return LzmaRet::ProgError,
    }

    let mut in_pos = 0;
    let mut out_pos = 0;
    let mut ret = LzmaRet::Ok;

    // 获取 next_out 的可变引用
    let mut next_out = strm.next_out.borrow_mut();

    let next_out_pos = strm.next_out_pos;

    // 执行编码/解码操作
    if let Some(next) = internal.next.as_mut() {
        if let Some(code) = next.code {
            ret = code(
                &mut next.coder.as_mut().unwrap(),
                &strm.next_in.to_vec(),
                &mut in_pos,
                strm.avail_in.get(),
                &mut next_out[next_out_pos as usize..],
                //&mut &next_out,
                &mut out_pos,
                strm.avail_out.get(),
                action,
            );
        }
    }

    // 更新输入状态
    if in_pos > 0 {
        strm.avail_in.set(strm.avail_in.get() - in_pos);
        strm.total_in.set(strm.total_in.get() + in_pos as u64);
        strm.next_in = &strm.next_in[in_pos..];
    }

    // 更新输出状态
    if out_pos > 0 {
        strm.avail_out.set(strm.avail_out.get() - out_pos);
        strm.total_out.set(strm.total_out.get() + out_pos as u64);

        // println!("output first 100 bytes: {:?}", &next_out[next_out_pos as usize..next_out.len().min(100)]);
        // 将已处理的数据从next_out中移除，保留未处理的数据
        // *next_out = next_out[out_pos..].to_vec();  //这个不需要了，只需要记录一下位置
        strm.next_out_pos = next_out_pos + out_pos as u32;
    }

    internal.avail_in = strm.avail_in.get();

    // 处理返回值
    match ret {
        LzmaRet::Ok => {
            if out_pos == 0 && in_pos == 0 {
                if internal.allow_buf_error {
                    LzmaRet::BufError
                } else {
                    internal.allow_buf_error = true;
                    LzmaRet::Ok
                }
            } else {
                internal.allow_buf_error = false;
                LzmaRet::Ok
            }
        }
        LzmaRet::RetInternal1 => {
            internal.allow_buf_error = false;
            LzmaRet::Ok
        }
        LzmaRet::SeekNeeded => {
            internal.allow_buf_error = false;
            if internal.sequence == Sequence::Finish {
                internal.sequence = Sequence::Run;
            }
            LzmaRet::SeekNeeded
        }
        LzmaRet::StreamEnd => {
            if internal.sequence == Sequence::SyncFlush
                || internal.sequence == Sequence::FullFlush
                || internal.sequence == Sequence::FullBarrier
            {
                internal.sequence = Sequence::Run;
            } else {
                internal.sequence = Sequence::End;
            }
            LzmaRet::StreamEnd
        }
        LzmaRet::NoCheck
        | LzmaRet::UnsupportedCheck
        | LzmaRet::GetCheck
        | LzmaRet::MemlimitError => {
            internal.allow_buf_error = false;
            ret
        }
        _ => {
            assert!(ret != LzmaRet::BufError);
            internal.sequence = Sequence::Error;
            println!("88888888888");
            ret
        }
    }
}

pub fn lzma_end(strm: Option<&mut LzmaStream>) {
    if let Some(stream) = strm {
        if let Some(mut internal) = stream.internal.borrow_mut().take() {
            if let Some(next) = internal.next.as_mut() {
                lzma_next_end(next);
            }
        }
    }
}

pub fn lzma_get_progress(strm: &mut LzmaStream, progress_in: &mut u64, progress_out: &mut u64) {
    if let Some(get_progress) = strm
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .next
        .as_mut()
        .unwrap()
        .get_progress
    {
        get_progress(
            strm.internal
                .borrow_mut()
                .as_mut()
                .unwrap()
                .next
                .as_mut()
                .unwrap()
                .coder
                .as_mut()
                .unwrap(),
            progress_in,
            progress_out,
        );
    } else {
        *progress_in = strm.total_in.get();
        *progress_out = strm.total_out.get();
    }
}

pub fn lzma_get_check(strm: &mut LzmaStream) -> LzmaCheck {
    if let Some(get_check) = strm
        .internal
        .borrow_mut()
        .as_mut()
        .unwrap()
        .next
        .as_mut()
        .unwrap()
        .get_check
    {
        get_check(
            strm.internal
                .borrow_mut()
                .as_mut()
                .unwrap()
                .next
                .as_mut()
                .unwrap()
                .coder
                .as_mut()
                .unwrap(),
        )
    } else {
        LzmaCheck::None
    }
}

pub fn lzma_memusage(strm: Option<&mut LzmaStream>) -> u64 {
    match strm {
        Some(strm) => {
            let mut memusage = 0;
            let mut old_memlimit = 0;

            if let Some(internal) = strm.internal.borrow_mut().as_mut() {
                if let Some(next) = internal.next.as_mut() {
                    if let Some(memconfig) = next.memconfig {
                        if memconfig(
                            next.coder.as_mut().unwrap(),
                            &mut memusage,
                            &mut old_memlimit,
                            0,
                        ) != LzmaRet::Ok
                        {
                            return 0;
                        }
                    }
                }
            }

            memusage
        }
        None => 0,
    }
}

pub fn lzma_memlimit_get(strm: Option<&mut LzmaStream>) -> u64 {
    match strm {
        Some(strm) => {
            let mut memusage = 0;
            let mut old_memlimit = 0;

            if let Some(internal) = strm.internal.borrow_mut().as_mut() {
                if let Some(next) = internal.next.as_mut() {
                    if let Some(memconfig) = next.memconfig {
                        if memconfig(
                            next.coder.as_mut().unwrap(),
                            &mut memusage,
                            &mut old_memlimit,
                            0,
                        ) != LzmaRet::Ok
                        {
                            return 0;
                        }
                    }
                }
            }

            old_memlimit
        }
        None => 0,
    }
}

pub fn lzma_memlimit_set(strm: &mut LzmaStream, mut new_memlimit: u64) -> LzmaRet {
    let mut old_memlimit = 0;
    let mut memusage = 0;

    if let Some(internal) = strm.internal.borrow_mut().as_mut() {
        if let Some(next) = internal.next.as_mut() {
            if let Some(memconfig) = next.memconfig {
                if new_memlimit == 0 {
                    new_memlimit = 1;
                }
                return memconfig(
                    next.coder.as_mut().unwrap(),
                    &mut memusage,
                    &mut old_memlimit,
                    new_memlimit,
                );
            } else {
                // memconfig 不存在
                return LzmaRet::ProgError;
            }
        }
    }

    // strm.internal 或 next 不存在
    LzmaRet::ProgError
}

pub const LZMA_SUPPORTED_FLAGS: u32 = LZMA_TELL_NO_CHECK
    | LZMA_TELL_UNSUPPORTED_CHECK
    | LZMA_TELL_ANY_CHECK
    | LZMA_IGNORE_CHECK
    | LZMA_CONCATENATED
    | LZMA_FAIL_FAST;
