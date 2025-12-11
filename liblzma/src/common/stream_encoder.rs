/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use std::os::raw::c_void;

use libc::memcpy;
use num_enum::TryFromPrimitive;

use crate::{
    api::{
        LzmaAction, LzmaBlock, LzmaCheck, LzmaFilter, LzmaRet, LzmaStream, LzmaStreamFlags,
        LZMA_BLOCK_HEADER_SIZE_MAX, LZMA_FILTERS_MAX, LZMA_STREAM_HEADER_SIZE, LZMA_VLI_UNKNOWN,
    },
    common::{lzma_block_unpadded_size, lzma_index_append, NextCoderInitFunction, LZMA_ACTION_MAX},
};

use super::{
    lzma_block_encoder_init, lzma_block_header_encode, lzma_block_header_size, lzma_bufcpy,
    lzma_end, lzma_filters_copy, lzma_filters_free, lzma_index_encoder_init, lzma_index_end,
    lzma_index_init, lzma_index_size, lzma_next_end, lzma_stream_footer_encode,
    lzma_stream_header_encode, lzma_strm_init, CoderType, LzmaIndex, LzmaNextCoder,
};

#[repr(C)]
#[derive(Debug)]
pub struct LzmaStreamEncoder {
    /// 编码序列状态
    sequence: StreamSequence,

    /// 如果通过 stream_encoder_init() 或 stream_encoder_update()
    /// 已经初始化了块编码器，则为 true。这种情况下，
    /// 在 stream_encode() 中就不需要再次初始化。
    block_encoder_is_initialized: bool,

    /// 块编码器
    block_encoder: Box<LzmaNextCoder>,

    /// 块编码器的配置选项
    block_options: LzmaBlock,

    /// 当前使用的过滤器链
    filters: [LzmaFilter; LZMA_FILTERS_MAX + 1],

    /// 索引编码器。这个与块编码器是分开的，因为它不占用太多内存，
    /// 而且当使用相同的编码选项编码多个流时，我们可以避免重新分配内存。
    index_encoder: Box<LzmaNextCoder>,

    /// 用于保存块大小的索引
    index: Option<Box<LzmaIndex>>,

    /// 缓冲区的读取位置
    buffer_pos: usize,

    /// 缓冲区中的总字节数
    buffer_size: usize,
    /// 用于保存流头部、块头部和流尾部的缓冲区。
    /// 块头部具有最大的最大尺寸。
    buffer: [u8; LZMA_BLOCK_HEADER_SIZE_MAX as usize],
}

impl Clone for LzmaStreamEncoder {
    fn clone(&self) -> Self {
        LzmaStreamEncoder {
            sequence: self.sequence,
            block_encoder_is_initialized: self.block_encoder_is_initialized,
            block_encoder: Box::new(LzmaNextCoder::default()), // 创建新的默认实例
            block_options: self.block_options.clone(),
            filters: self.filters.clone(),
            index_encoder: Box::new(LzmaNextCoder::default()), // 创建新的默认实例
            index: self.index.clone(),
            buffer_pos: self.buffer_pos,
            buffer_size: self.buffer_size,
            buffer: self.buffer,
        }
    }
}

impl Default for LzmaStreamEncoder {
    fn default() -> Self {
        LzmaStreamEncoder {
            sequence: StreamSequence::default(), // 假设 StreamSequence 实现了 Default
            block_encoder_is_initialized: false,
            block_encoder: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            block_options: LzmaBlock::default(),               // 假设 LzmaBlock 实现了 Default
            filters: core::array::from_fn(|_| LzmaFilter::default()),
            index_encoder: Box::new(LzmaNextCoder::default()), // 假设 LzmaNextCoder 实现了 Default
            index: None,                                       // Option 类型默认是 None
            buffer_pos: 0,
            buffer_size: 0,
            buffer: [0; LZMA_BLOCK_HEADER_SIZE_MAX as usize], // 初始化为全零数组
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, TryFromPrimitive, Default)]
enum StreamSequence {
    /// 流头部
    #[default]
    StreamHeader,
    /// 块初始化
    BlockInit,
    /// 块头部
    BlockHeader,
    /// 块编码
    BlockEncode,
    /// 索引编码
    IndexEncode,
    /// 流尾部
    StreamFooter,
}

fn block_encoder_init(coder: &mut LzmaStreamEncoder) -> LzmaRet {
    // 准备块选项。虽然块编码器不需要初始化 compressed_size、
    // uncompressed_size 和 header_size，但在这里初始化是个好主意，
    // 因为这样我们可以检查是否有人给了我们一个不能在块/流中使用的过滤器 ID。
    coder.block_options.compressed_size = LZMA_VLI_UNKNOWN;
    coder.block_options.uncompressed_size = LZMA_VLI_UNKNOWN;

    // 检查块头部大小是否有效
    let mut ret = lzma_block_header_size(&mut coder.block_options);
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 初始化实际的块编码器
    ret = lzma_block_encoder_init(&mut coder.block_encoder, &coder.block_options);
    ret
}

fn stream_encode(
    coder_ptr: &mut CoderType,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // println!("======== stream_encode");
    let coder = match coder_ptr {
        CoderType::StreamEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    // println!("stream_encode coder {:#?}", coder);
    // 主循环
    while *out_pos < out_size {
        // println!("coder.sequence {:#?}", coder.sequence);
        match coder.sequence {
            StreamSequence::StreamHeader
            | StreamSequence::BlockHeader
            | StreamSequence::StreamFooter => {
                lzma_bufcpy(
                    &mut coder.buffer,
                    &mut coder.buffer_pos,
                    coder.buffer_size,
                    output,
                    out_pos,
                    out_size,
                );

                if coder.buffer_pos < coder.buffer_size {
                    return LzmaRet::Ok;
                }

                if coder.sequence == StreamSequence::StreamFooter {
                    return LzmaRet::StreamEnd;
                }

                coder.buffer_pos = 0;
                let tmp = coder.sequence as u32;
                coder.sequence = StreamSequence::try_from(tmp + 1).unwrap();
            }

            StreamSequence::BlockInit => {
                // 检查是否需要完成处理
                if *in_pos == in_size {
                    // 如果我们被要求刷新或完成当前块，
                    // 由于没有要做的事情，立即返回 LZMA_STREAM_END
                    if action.clone() != LzmaAction::Finish {
                        return if action.clone() == LzmaAction::Run {
                            LzmaRet::Ok
                        } else {
                            LzmaRet::StreamEnd
                        };
                    }
                    let ret: LzmaRet = {
                        // let index_encoder = &mut coder.index_encoder;
                        lzma_index_encoder_init(
                            &mut coder.index_encoder,
                            coder.index.as_mut().unwrap(),
                        )
                    };

                    if ret != LzmaRet::Ok {
                        return ret;
                    }

                    coder.sequence = StreamSequence::IndexEncode;
                    continue;
                }

                // 初始化块编码器，除非它已经被 stream_encoder_init()
                // 或 stream_encoder_update() 初始化。
                if !coder.block_encoder_is_initialized {
                    // 注意：这里也存在借用冲突
                    // TODO: 重构此代码以避免借用冲突
                    let ret = block_encoder_init(coder);
                    if ret != LzmaRet::Ok {
                        return ret;
                    }
                }

                // 设为 false，这样我们不会在下一个块跳过初始化
                coder.block_encoder_is_initialized = false;

                // 编码块头部。这不应该失败，因为我们已经初始化了块编码器
                if lzma_block_header_encode(&coder.block_options, &mut coder.buffer) != LzmaRet::Ok
                {
                    return LzmaRet::ProgError;
                }

                coder.buffer_size = coder.block_options.header_size as usize;
                coder.sequence = StreamSequence::BlockHeader;
            }

            StreamSequence::BlockEncode => {
                static ACTION_CONVERT: [LzmaAction; LZMA_ACTION_MAX + 1] = [
                    LzmaAction::Run,
                    LzmaAction::SyncFlush,
                    LzmaAction::Finish,
                    LzmaAction::Finish,
                    LzmaAction::Finish,
                ];

                let mut ret = LzmaRet::Ok;
                // println!("=========== 111111111111");
                if let Some(code) = coder.block_encoder.code {
                    ret = code(
                        &mut coder.block_encoder.coder.as_mut().unwrap(),
                        input,
                        in_pos,
                        in_size,
                        output,
                        out_pos,
                        out_size,
                        ACTION_CONVERT[action.clone() as usize].clone(),
                    );
                }
                // 这段代码是将block内容更新到block_options中的
                if let Some(CoderType::BlockEncoder(block_coder)) =
                    coder.block_encoder.coder.as_mut()
                {
                    if let Some(block) = block_coder.get_block_info() {
                        coder.block_options = block;
                    }
                }

                // println!("=========== 2222222222");
                if ret != LzmaRet::StreamEnd || action.clone() == LzmaAction::SyncFlush {
                    return ret;
                }

                // 添加新的索引记录
                let unpadded_size: u64 = lzma_block_unpadded_size(&coder.block_options);
                assert!(unpadded_size != 0);

                // 注意：这里也存在借用冲突
                // 使用临时变量来避免多次可变借用
                // let index: Box<LzmaIndex> = coder.index.as_mut().unwrap().clone();
                let ret = lzma_index_append(
                    coder.index.as_mut().map(|b| b.as_mut()).unwrap(),
                    unpadded_size,
                    coder.block_options.uncompressed_size,
                );
                if ret != LzmaRet::Ok {
                    return ret;
                }

                coder.sequence = StreamSequence::BlockInit;
            }

            StreamSequence::IndexEncode => {
                // 调用索引编码器。它不需要任何输入，所以那些指针可以是 NULL
                let mut ret = LzmaRet::Ok;
                // 注意：这里也存在借用冲突
                // TODO: 重构此代码以避免借用冲突
                if let Some(code) = coder.index_encoder.code {
                    ret = code(
                        &mut coder.index_encoder.coder.as_mut().unwrap(),
                        &mut Vec::new(),
                        &mut 0,
                        0,
                        output,
                        out_pos,
                        out_size,
                        LzmaAction::Run,
                    );
                }
                if ret != LzmaRet::StreamEnd {
                    return ret;
                }

                // 将流尾部编码到 coder->buffer
                // 注意：这里也存在借用冲突
                // TODO: 重构此代码以避免借用冲突
                let mut stream_flags = LzmaStreamFlags {
                    version: 0,
                    backward_size: lzma_index_size(coder.index.as_ref().unwrap()), // lzma_index_size(coder.index.as_ref().unwrap()),
                    check: coder.block_options.check.clone(),
                    ..Default::default()
                };

                if lzma_stream_footer_encode(&mut stream_flags, &mut coder.buffer) != LzmaRet::Ok {
                    return LzmaRet::ProgError;
                }

                coder.buffer_size = LZMA_STREAM_HEADER_SIZE;
                coder.sequence = StreamSequence::StreamFooter;
            }
        }
    }

    LzmaRet::Ok
}

/// 结束流编码器并释放资源
fn stream_encoder_end(coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::StreamEncoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };

    lzma_next_end(&mut coder.block_encoder);
    lzma_next_end(&mut coder.index_encoder);
    lzma_index_end(coder.index.as_mut().unwrap());

    lzma_filters_free(&mut coder.filters);
}

/// 更新流编码器的过滤器链
fn stream_encoder_update(
    coder_ptr: &mut CoderType,
    filters: Option<&[LzmaFilter]>,
    reversed_filters: &[LzmaFilter],
) -> LzmaRet {
    let coder = match coder_ptr {
        CoderType::StreamEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    let mut ret: LzmaRet;
    let mut temp: [LzmaFilter; LZMA_FILTERS_MAX + 1] =
        core::array::from_fn(|_| LzmaFilter::default());
    ret = lzma_filters_copy(filters.unwrap(), &mut temp);
    if ret != LzmaRet::Ok {
        return ret;
    }

    if coder.sequence <= StreamSequence::BlockInit {
        // 没有未完成的块等待完成，因此我们可以更改整个过滤器链。
        // 首先尝试使用新链初始化块编码器。

        coder.block_encoder_is_initialized = false;
        coder.block_options.filters = (temp).to_vec();
        ret = block_encoder_init(coder);
        // coder.block_options.filters = (coder.filters).to_vec();
        if ret != LzmaRet::Ok {
            lzma_filters_free(&mut temp);
            return ret;
        }
        coder.block_encoder_is_initialized = true;
    } else if coder.sequence <= StreamSequence::BlockEncode {
        // 我们在块的中间。尝试仅更新过滤器特定的选项
        let mut ret = LzmaRet::Ok;
        if let Some(update) = coder.block_encoder.update {
            ret = update(
                &mut coder.block_encoder.coder.as_mut().unwrap(),
                filters,
                reversed_filters,
            );
        }
        if ret != LzmaRet::Ok {
            lzma_filters_free(&mut temp);
            return ret;
        }
    } else {
        // 尝试在我们已经编码索引或流尾部时更新过滤器链。
        ret = LzmaRet::ProgError;
        if ret != LzmaRet::Ok {
            lzma_filters_free(&mut temp);
            return ret;
        }
    };

    lzma_filters_free(&mut coder.filters);

    coder.filters = temp;
    coder.block_options.filters = (coder.filters).to_vec();

    LzmaRet::Ok
}

fn stream_encoder_init(
    next: &mut LzmaNextCoder,
    filters: Option<&[LzmaFilter]>,
    check: LzmaCheck,
) -> LzmaRet {
    // lzma_next_coder_init(&stream_encoder_init, next, allocator);
    if next.init != Some(NextCoderInitFunction::StreamEncoder(stream_encoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::StreamEncoder(stream_encoder_init));

    // 检查过滤器是否为空
    let filters = match filters {
        Some(f) => f,
        None => {
            return LzmaRet::ProgError;
        }
    };

    let mut coder: &mut LzmaStreamEncoder = &mut LzmaStreamEncoder::default();
    // 创建或获取编码器
    if next.coder.is_none() {
        let mut _coder = LzmaStreamEncoder::default();
        // 初始化编码器
        _coder.filters[0].id = LZMA_VLI_UNKNOWN;
        _coder.block_encoder = Box::new(LzmaNextCoder::default());
        _coder.index_encoder = Box::new(LzmaNextCoder::default());
        _coder.index = None;

        next.coder = Some(CoderType::StreamEncoder(_coder));
        next.code = Some(stream_encode);
        next.end = Some(stream_encoder_end);
        next.update = Some(stream_encoder_update);
    }
    coder = match next.coder.as_mut().unwrap() {
        CoderType::StreamEncoder(c) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.sequence = StreamSequence::StreamHeader;
    coder.block_options.version = 0;
    coder.block_options.check = check.clone();

    // 初始化索引
    if let Some(ref mut index) = coder.index {
        lzma_index_end(index);
    }
    coder.index = lzma_index_init().map(|arc| Box::new(arc.lock().unwrap().clone()));

    if coder.index.is_none() {
        return LzmaRet::MemError;
    }

    // 编码流头部
    let stream_flags = LzmaStreamFlags {
        version: 0,
        check: check.clone(),
        ..Default::default()
    };

    let ret = lzma_stream_header_encode(&stream_flags, &mut coder.buffer);
    if ret != LzmaRet::Ok {
        return ret;
    }

    coder.buffer_pos = 0;
    coder.buffer_size = LZMA_STREAM_HEADER_SIZE;

    // 创建新的coder并设置到next.coder中
    let new_coder = LzmaStreamEncoder {
        sequence: coder.sequence,
        block_encoder_is_initialized: coder.block_encoder_is_initialized,
        block_encoder: Box::new(LzmaNextCoder::default()),
        block_options: coder.block_options.clone(),
        filters: coder.filters.clone(),
        index_encoder: Box::new(LzmaNextCoder::default()),
        index: coder.index.clone(),
        buffer_pos: coder.buffer_pos,
        buffer_size: coder.buffer_size,
        buffer: coder.buffer,
    };

    next.coder = Some(CoderType::StreamEncoder(new_coder));

    return stream_encoder_update(next.coder.as_mut().unwrap(), Some(filters), &[]);
}

/// 公共 API 函数：初始化 LZMA 流编码器
pub fn lzma_stream_encoder(
    strm: &mut LzmaStream,
    filters: &[LzmaFilter],
    check: LzmaCheck,
) -> LzmaRet {
    // lzma_next_strm_init(stream_encoder_init, strm, filters, check);
    // stream_encoder_init(&(strm)->internal->next, (strm)->allocator, filters, check);
    let ret_: LzmaRet = lzma_strm_init(Some(strm));
    if ret_ != LzmaRet::Ok {
        return ret_;
    }
    let ret_0: LzmaRet = stream_encoder_init(
        &mut strm
            .internal
            .borrow_mut()
            .as_mut()
            .unwrap()
            .next
            .as_mut()
            .unwrap(),
        Some(filters),
        check,
    );

    if ret_0 != LzmaRet::Ok {
        lzma_end(Some(strm));
        return ret_0;
    }

    // 设置支持的操作，分离借用来避免临时值问题
    let mut internal_borrow = strm.internal.borrow_mut();
    let supported = &mut internal_borrow.as_mut().unwrap().supported_actions;
    supported[LzmaAction::Run as usize] = true;
    supported[LzmaAction::SyncFlush as usize] = true;
    supported[LzmaAction::FullFlush as usize] = true;
    supported[LzmaAction::FullBarrier as usize] = true;
    supported[LzmaAction::Finish as usize] = true;

    LzmaRet::Ok
}
