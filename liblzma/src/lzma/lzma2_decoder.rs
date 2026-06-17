/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaVli},
    common::{CoderType, LzmaFilterInfo, LzmaNextCoder},
    lz::{
        dict_reset, dict_write, lzma_lz_decoder_init, LzCoderType, LzmaDict, LzmaLzDecoder,
        LzmaLzDecoderOptions,
    },
};

use super::{
    lzma_lzma_decoder_create, lzma_lzma_decoder_memusage_nocheck, lzma_lzma_lclppb_decode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Sequence {
    #[default]
    Control,
    Uncompressed1,
    Uncompressed2,
    Compressed0,
    Compressed1,
    Properties,
    Lzma,
    Copy,
}

#[derive(Debug)]
pub struct LzmaLzma2Decoder {
    sequence: Sequence,
    next_sequence: Sequence,
    control: u8,
    lzma: Box<LzmaLzDecoder>,
    uncompressed_size: usize,
    compressed_size: usize,
    need_properties: bool,
    need_dictionary_reset: bool,
    options: LzmaOptionsLzma,
}

impl Default for LzmaLzma2Decoder {
    fn default() -> Self {
        LzmaLzma2Decoder {
            sequence: Sequence::default(),      // 假设 Sequence 实现了 Default
            next_sequence: Sequence::default(), // 假设 Sequence 实现了 Default
            control: 0,
            lzma: Box::new(LzmaLzDecoder::default()), // 假设 LzmaLzDecoder 实现了 Default
            uncompressed_size: 0,
            compressed_size: 0,
            need_properties: false,
            need_dictionary_reset: false,
            options: LzmaOptionsLzma::default(), // 假设 LzmaOptionsLzma 实现了 Default
        }
    }
}

fn lzma2_decode(
    coder_ptr: &mut LzCoderType,
    dict: &mut LzmaDict,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    // let coder = coder_ptr.downcast_mut::<LzmaLzma2Decoder>().unwrap();
    let coder = match coder_ptr {
        LzCoderType::Lzma2Decoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    while *in_pos < in_size || coder.sequence == Sequence::Lzma {
        match coder.sequence {
            Sequence::Control => {
                let control = input[*in_pos];
                *in_pos += 1;
                coder.control = control;

                if control == 0x00 {
                    return LzmaRet::StreamEnd;
                }

                if control >= 0xE0 || control == 1 {
                    coder.need_properties = true;
                    coder.need_dictionary_reset = true;
                } else if coder.need_dictionary_reset {
                    return LzmaRet::DataError;
                }

                if control >= 0x80 {
                    coder.uncompressed_size = ((control & 0x1F) as usize) << 16;
                    coder.sequence = Sequence::Uncompressed1;

                    if control >= 0xC0 {
                        coder.need_properties = false;
                        coder.next_sequence = Sequence::Properties;
                    } else if coder.need_properties {
                        return LzmaRet::DataError;
                    } else {
                        coder.next_sequence = Sequence::Lzma;
                        if control >= 0xA0 {
                            if let Some(reset) = coder.lzma.reset {
                                reset(
                                    coder.lzma.coder.as_mut().unwrap(),
                                    &LzmaOptionsType::LzmaOptionsLzma(coder.options.clone()),
                                );
                            }
                        }
                    }
                } else {
                    if control > 2 {
                        return LzmaRet::DataError;
                    }
                    coder.sequence = Sequence::Compressed0;
                    coder.next_sequence = Sequence::Copy;
                }

                if coder.need_dictionary_reset {
                    coder.need_dictionary_reset = false;
                    dict_reset(dict);
                    return LzmaRet::Ok;
                }
            }

            Sequence::Uncompressed1 => {
                coder.uncompressed_size += (input[*in_pos] as usize) << 8;
                *in_pos += 1;
                coder.sequence = Sequence::Uncompressed2;
            }

            Sequence::Uncompressed2 => {
                coder.uncompressed_size += input[*in_pos] as usize + 1;
                *in_pos += 1;
                coder.sequence = Sequence::Compressed0;
                if let Some(set_uncompressed) = coder.lzma.set_uncompressed {
                    set_uncompressed(
                        coder.lzma.coder.as_mut().unwrap(),
                        coder.uncompressed_size as u64,
                        false,
                    );
                }
            }

            Sequence::Compressed0 => {
                coder.compressed_size = (input[*in_pos] as usize) << 8;
                *in_pos += 1;
                coder.sequence = Sequence::Compressed1;
            }

            Sequence::Compressed1 => {
                coder.compressed_size += input[*in_pos] as usize + 1;
                *in_pos += 1;
                coder.sequence = coder.next_sequence;
            }

            Sequence::Properties => {
                if lzma_lzma_lclppb_decode(&mut coder.options, input[*in_pos]) {
                    return LzmaRet::DataError;
                }
                *in_pos += 1;

                if let Some(reset) = coder.lzma.reset {
                    reset(
                        coder.lzma.coder.as_mut().unwrap(),
                        &LzmaOptionsType::LzmaOptionsLzma(coder.options.clone()),
                    );
                }

                coder.sequence = Sequence::Lzma;
            }

            Sequence::Lzma => {
                let in_start = *in_pos;
                let mut ret: LzmaRet = LzmaRet::Ok;
                if let Some(code) = coder.lzma.code {
                    ret = code(
                        coder.lzma.coder.as_mut().unwrap(),
                        dict,
                        input,
                        in_pos,
                        in_size,
                    );
                }

                let in_used = *in_pos - in_start;
                if in_used > coder.compressed_size {
                    return LzmaRet::DataError;
                }

                coder.compressed_size -= in_used;

                if ret != LzmaRet::StreamEnd {
                    return ret;
                }

                if coder.compressed_size != 0 {
                    return LzmaRet::DataError;
                }

                coder.sequence = Sequence::Control;
            }

            Sequence::Copy => {
                dict_write(dict, input, in_pos, in_size, &mut coder.compressed_size);
                if coder.compressed_size != 0 {
                    return LzmaRet::Ok;
                }
                coder.sequence = Sequence::Control;
            }
            _ => {
                assert!(false);
                return LzmaRet::ProgError;
            }
        }
    }

    LzmaRet::Ok
}

fn lzma2_decoder_end(coder_ptr: &mut LzCoderType) {
    let coder = match coder_ptr {
        LzCoderType::Lzma2Decoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    assert!(coder.lzma.end.is_none());
}

pub fn lzma2_decoder_init(
    lz: &mut LzmaLzDecoder,
    id: LzmaVli,
    options: &LzmaOptionsType,
    lz_options: &mut LzmaLzDecoderOptions,
) -> LzmaRet {
    // let mut coder = lz.coder.downcast_mut::<LzmaLzma2Decoder>();
    // let coder: &mut LzmaLzma2Decoder = lz.coder.as_mut().unwrap().as_mut().unwrap();

    if lz.coder.is_none() {
        let coder_ = LzmaLzma2Decoder::default();

        lz.coder = Some(LzCoderType::Lzma2Decoder(coder_));
        lz.code = Some(lzma2_decode);
        lz.end = Some(lzma2_decoder_end);
    }

    let coder = match lz.coder.as_mut().unwrap() {
        LzCoderType::Lzma2Decoder(ref mut c) => c,
        _ => return LzmaRet::ProgError,
    };

    coder.lzma = Box::new(LzmaLzDecoder::default());

    let options = options.as_lzma_options_lzma().unwrap();
    coder.sequence = Sequence::Control;
    coder.need_properties = true;
    coder.need_dictionary_reset = options.preset_dict.is_none() || options.preset_dict_size == 0;

    lzma_lzma_decoder_create(&mut coder.lzma, options, lz_options)
}

pub fn lzma_lzma2_decoder_init(next: &mut LzmaNextCoder, filters: &[LzmaFilterInfo]) -> LzmaRet {
    assert!(filters[1].init.is_none());

    lzma_lz_decoder_init(next, filters, lzma2_decoder_init);
    LzmaRet::Ok
}

pub fn lzma_lzma2_decoder_memusage(options: &LzmaOptionsType) -> u64 {
    std::mem::size_of::<LzmaLzma2Decoder>() as u64 + lzma_lzma_decoder_memusage_nocheck(options)
}

pub fn lzma_lzma2_props_decode(
    props: &[u8],
    props_size: usize,
) -> (LzmaRet, Option<LzmaOptionsType>) {
    if props_size != 1 {
        return (LzmaRet::OptionsError, None);
    }

    if props[0] & 0xC0 != 0 {
        return (LzmaRet::OptionsError, None);
    }

    if props[0] > 40 {
        return (LzmaRet::OptionsError, None);
    }

    let mut opt = LzmaOptionsLzma::default();

    if props[0] == 40 {
        opt.dict_size = u32::MAX;
    } else {
        opt.dict_size = (2 | (props[0] & 1)) as u32;
        opt.dict_size <<= props[0] / 2 + 11;
    }

    opt.preset_dict = None;
    opt.preset_dict_size = 0;

    let options = LzmaOptionsType::LzmaOptionsLzma(opt);
    (LzmaRet::Ok, Some(options))
}
