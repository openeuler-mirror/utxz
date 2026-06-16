/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{
        lzma_set_ext_size, LzmaAction, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream,
        LzmaVli, LZMA_FILTER_LZMA1EXT, LZMA_VLI_MAX,
    },
    common::{lzma_next_filter_init, LzmaFilterInfo, NextCoderInitFunction},
    lzma::{lzma_lzma_decoder_init, lzma_lzma_lclppb_decode},
};

use super::{lzma_end, lzma_next_end, lzma_strm_init, CoderType, LzmaNextCoder};

/// MicroLZMA 解码器状态
#[derive(Debug, Default)]
pub struct LzmaMicrolzmaDecoder {
    /// LZMA1 解码器
    lzma: Box<LzmaNextCoder>,

    /// 应用程序提供的流的压缩大小。
    /// 这个值必须完全正确。
    ///
    /// 当读取输入时，这个值会递减。
    comp_size: u64,

    /// 应用程序提供的流的解压大小。
    /// 如果 uncomp_size_is_exact 为 false，这个值可能小于实际解压大小。
    ///
    /// 当产生输出时，这个值会递减。
    uncomp_size: LzmaVli,

    /// 应用程序提供的 LZMA 字典大小
    dict_size: u32,

    /// 如果为 true，则表示确切的解压大小已知。
    /// 如果为 false，uncomp_size 可能小于实际解压大小；
    /// uncomp_size 永远不能大于实际解压大小。
    uncomp_size_is_exact: bool,

    /// 一旦处理了 MicroLZMA 流的第一个字节，则为 true。
    props_decoded: bool,
}

/// 解码 MicroLZMA 流
fn microlzma_decode(
    coder_ptr: &mut CoderType,

    in_data: &[u8],
    in_pos: &mut usize,
    mut in_size: usize,
    out_data: &mut [u8],
    out_pos: &mut usize,
    mut out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_ref::<LzmaMicrolzmaDecoder>().unwrap();
    let coder = match coder_ptr {
        CoderType::MicroLzamDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };
    // 记录输入流的起始位置，以便后续更新 comp_size
    let in_start = *in_pos;

    // 记录输出流的起始位置，以便后续更新 uncomp_size
    let out_start = *out_pos;

    // 限制输入数据量，确保解码器不会读取超过 comp_size 的内容
    if in_size - *in_pos > coder.comp_size as usize {
        in_size = *in_pos + coder.comp_size as usize;
    }

    // 当解压的确切大小不确定时，限制输出空间，以防止解码器读取过多数据
    if !coder.uncomp_size_is_exact && out_size - *out_pos > coder.uncomp_size as usize {
        out_size = *out_pos + coder.uncomp_size as usize;
    }

    if !coder.props_decoded {
        // 解码属性字节时，至少需要一个字节的输入数据
        if *in_pos >= in_size {
            return LzmaRet::Ok;
        }

        // 配置 LZMA 解码器选项
        let mut options = LzmaOptionsLzma {
            dict_size: coder.dict_size,
            preset_dict: None,
            preset_dict_size: 0,
            ext_flags: 0,           // 不允许 EOPM，当大小已知时
            ext_size_low: u32::MAX, // 默认未知大小
            ext_size_high: u32::MAX,
            ..Default::default()
        };

        if coder.uncomp_size_is_exact {
            lzma_set_ext_size(&mut options, coder.uncomp_size);
        }

        // 属性以逐位取反的方式存储
        if lzma_lzma_lclppb_decode(&mut options, !in_data[*in_pos]) {
            return LzmaRet::OptionsError;
        }

        *in_pos += 1;

        // 初始化解码器
        // let mut tmp: Box<dyn std::any::Any> = Box::new(options);
        let filters: [LzmaFilterInfo; 2] = [
            LzmaFilterInfo {
                id: LZMA_FILTER_LZMA1EXT,
                init: Some(lzma_lzma_decoder_init),
                options: Some(LzmaOptionsType::LzmaOptionsLzma(options)),
            },
            LzmaFilterInfo {
                id: 0,
                init: None,
                options: None,
            },
        ];

        let ret = lzma_next_filter_init(&mut coder.lzma, &filters);
        if ret != LzmaRet::Ok {
            return ret;
        }

        // 向 LZMA 解码器传递一个虚拟的 0x00 字节，因为它期望解码的第一个字节是这个
        let dummy_in: u8 = 0;
        let mut dummy_in_pos = 0;
        let mut ret: LzmaRet = LzmaRet::Ok;
        if let Some(code) = coder.lzma.code {
            ret = code(
                coder.lzma.coder.as_mut().unwrap(),
                &mut [dummy_in].to_vec(),
                &mut dummy_in_pos,
                1,
                out_data,
                out_pos,
                out_size,
                LzmaAction::Run,
            );
            if ret != LzmaRet::Ok {
                return LzmaRet::ProgError;
            }
        }

        assert_eq!(dummy_in_pos, 1);
        coder.props_decoded = true;
    }

    // 正常的 LZMA 解码过程
    let mut ret: LzmaRet = LzmaRet::Ok;
    if let Some(code) = coder.lzma.code {
        ret = code(
            coder.lzma.coder.as_mut().unwrap(),
            in_data,
            in_pos,
            in_size,
            out_data,
            out_pos,
            out_size,
            action,
        );
    }

    // 更新剩余的压缩大小
    assert!(coder.comp_size >= (*in_pos - in_start) as u64);
    coder.comp_size -= (*in_pos - in_start) as u64;

    if coder.uncomp_size_is_exact {
        // 完整流解压后，压缩大小必须匹配
        if ret == LzmaRet::StreamEnd && coder.comp_size != 0 {
            ret = LzmaRet::DataError;
        }
    } else {
        // 更新剩余输出大小
        assert!(coder.uncomp_size >= (*out_pos - out_start) as u64);
        coder.uncomp_size -= (*out_pos - out_start) as u64;

        // - 我们不能得到 LZMA_STREAM_END，因为流不应该有 EOPM。
        // - 使用 uncomp_size 来决定何时返回 LZMA_STREAM_END。
        if ret == LzmaRet::StreamEnd {
            ret = LzmaRet::DataError;
        } else if coder.uncomp_size == 0 {
            ret = LzmaRet::StreamEnd;
        }
    }

    ret
}

/// 结束 MicroLZMA 解码器
fn microlzma_decoder_end(mut coder_ptr: &mut CoderType) {
    let coder = match coder_ptr {
        CoderType::MicroLzamDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.lzma);
}

/// 初始化 MicroLZMA 解码器
fn microlzma_decoder_init(
    next: &mut LzmaNextCoder,

    comp_size: u64,
    uncomp_size: u64,
    uncomp_size_is_exact: bool,
    dict_size: u32,
) -> LzmaRet {
    // 调用 LZMA 解码器初始化
    // lzma_next_coder_init(&microlzma_decoder_init, next, allocator);
    if next.init
        != Some(NextCoderInitFunction::MicroLzamDecoder(
            microlzma_decoder_init,
        ))
    {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::MicroLzamDecoder(
        microlzma_decoder_init,
    ));

    let coder = &mut LzmaMicrolzmaDecoder::default();
    if next.coder.is_none() {
        let coder_ = LzmaMicrolzmaDecoder::default();

        next.coder = Some(CoderType::MicroLzamDecoder(coder_));
        next.code = Some(microlzma_decode);
        next.end = Some(microlzma_decoder_end);
        coder.lzma = Box::new(LzmaNextCoder::default());
    }

    // 检查 uncomp_size 是否超过最大值
    if uncomp_size > LZMA_VLI_MAX {
        return LzmaRet::OptionsError;
    }

    // 初始化解码器参数
    coder.comp_size = comp_size;
    coder.uncomp_size = uncomp_size;
    coder.uncomp_size_is_exact = uncomp_size_is_exact;
    coder.dict_size = dict_size;
    coder.props_decoded = false;

    LzmaRet::Ok
}

/// MicroLZMA 解码器初始化并启动流
fn lzma_microlzma_decoder(
    strm: &mut LzmaStream,
    comp_size: u64,
    uncomp_size: u64,
    uncomp_size_is_exact: bool,
    dict_size: u32,
) -> LzmaRet {
    // 初始化流
    // lzma_next_strm_init(
    //     microlzma_decoder_init,
    //     strm,
    //     comp_size,
    //     uncomp_size,
    //     uncomp_size_is_exact,
    //     dict_size,
    // );
    let ret: LzmaRet = lzma_strm_init(Some(strm));
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 避免借用冲突的初始化
    let init_ret = match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                if let Some(ref mut next) = internal.next {
                    microlzma_decoder_init(
                        next,
                        comp_size,
                        uncomp_size,
                        uncomp_size_is_exact,
                        dict_size,
                    )
                } else {
                    LzmaRet::ProgError
                }
            } else {
                LzmaRet::ProgError
            }
        }
        Err(_) => LzmaRet::ProgError,
    };

    if init_ret != LzmaRet::Ok {
        lzma_end(Some(strm));
        return init_ret;
    }

    // 设置支持的操作
    match strm.internal.try_borrow_mut() {
        Ok(mut internal_ref) => {
            if let Some(ref mut internal) = internal_ref.as_mut() {
                internal.supported_actions[LzmaAction::Run as usize] = true;
                internal.supported_actions[LzmaAction::Finish as usize] = true;
            }
        }
        Err(_) => return LzmaRet::ProgError,
    }

    LzmaRet::Ok
}
