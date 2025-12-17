/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_max;

use crate::{
    api::{
        LzmaAction, LzmaCheck, LzmaRet, LzmaStream, LZMA_CONCATENATED, LZMA_TELL_ANY_CHECK,
        LZMA_TELL_NO_CHECK,
    },
    common::{
        lzma_alone_decoder_init, lzma_stream_decoder_init, NextCoderInitFunction,
        LZMA_SUPPORTED_FLAGS,
    },
};

use super::{
    lzma_end, lzma_next_end, lzma_strm_init, CoderType, LzmaNextCoder, LZMA_MEMUSAGE_BASE,
};

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

fn auto_decode(
    coder_ptr: &mut CoderType,

    in_: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = unsafe { &mut *(coder_ptr as *mut LzmaAutoCoder) };
    // let coder = coder_ptr;

    let coder = match coder_ptr {
        CoderType::AutoDecoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    match coder.sequence {
        Sequence::SeqInit => {
            if *in_pos >= in_size {
                return LzmaRet::Ok;
            }

            coder.sequence = Sequence::SeqCode;

            let current_byte = in_[*in_pos];
            if current_byte == 0xFD {
                let ret = lzma_stream_decoder_init(&mut coder.next, coder.memlimit, coder.flags);
                if ret != LzmaRet::Ok {
                    return ret;
                }
            } else {
                let ret = lzma_alone_decoder_init(&mut coder.next, coder.memlimit, true);
                if ret != LzmaRet::Ok {
                    return ret;
                }

                if coder.flags & LZMA_TELL_NO_CHECK != 0 {
                    return LzmaRet::NoCheck;
                }

                if coder.flags & LZMA_TELL_ANY_CHECK != 0 {
                    return LzmaRet::GetCheck;
                }
            }
        }

        Sequence::SeqCode => {
            let mut ret: LzmaRet = LzmaRet::Ok;
            if let Some(code) = coder.next.code {
                ret = code(
                    &mut coder.next.coder.as_mut().unwrap(),
                    in_,
                    in_pos,
                    in_size,
                    out,
                    out_pos,
                    out_size,
                    action,
                );
            } else {
                return LzmaRet::ProgError;
            }

            if (ret != LzmaRet::StreamEnd) || (coder.flags & LZMA_CONCATENATED == 0) {
                return ret;
            }

            coder.sequence = Sequence::SeqFinish;
        }

        Sequence::SeqFinish => {
            if *in_pos < in_size {
                return LzmaRet::DataError;
            }

            return if action == LzmaAction::Finish {
                LzmaRet::StreamEnd
            } else {
                LzmaRet::Ok
            };
        }

        _ => {
            assert!(false);
            return LzmaRet::ProgError;
        }
    }

    LzmaRet::Ok
}

fn auto_decoder_end(coder_ptr: &mut CoderType) {
    // let coder = unsafe { &mut *(coder_ptr as *mut LzmaAutoCoder) };
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::AutoDecoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    lzma_next_end(&mut coder.next);
}

fn auto_decoder_get_check(coder_ptr: &mut CoderType) -> LzmaCheck {
    // let coder = unsafe { &*(coder_ptr as *const LzmaAutoCoder) };
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::AutoDecoder(ref mut c) => c,
        _ => return LzmaCheck::None, // 如果不是 AloneDecoder 类型，则返回错误
    };
    if coder.next.get_check.is_none() {
        LzmaCheck::None
    } else {
        if let Some(get_check) = coder.next.get_check {
            get_check(&mut coder.next.coder.as_mut().unwrap())
        } else {
            LzmaCheck::None
        }
    }
}

fn auto_decoder_memconfig(
    coder_ptr: &mut CoderType,
    memusage: &mut u64,
    old_memlimit: &mut u64,
    new_memlimit: u64,
) -> LzmaRet {
    // let mut coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::AutoDecoder(ref mut c) => c,
        _ => return LzmaRet::MemError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    let mut ret: LzmaRet = LzmaRet::Ok;

    if coder.next.memconfig.is_some() {
        if let Some(memconfig) = coder.next.memconfig {
            ret = memconfig(
                coder.next.coder.as_mut().unwrap(),
                memusage,
                old_memlimit,
                new_memlimit,
            )
        }
    } else {
        *memusage = LZMA_MEMUSAGE_BASE;
        *old_memlimit = coder.memlimit;

        ret = LzmaRet::Ok;
        if new_memlimit != 0 && new_memlimit < *memusage {
            ret = LzmaRet::MemlimitError
        }
    }

    if ret == LzmaRet::Ok && new_memlimit != 0 {
        coder.memlimit = new_memlimit;
    }

    ret
}

fn auto_decoder_init(next: &mut LzmaNextCoder, memlimit: u64, flags: u32) -> LzmaRet {
    // lzma_next_coder_init!(auto_decoder_init, next, Some(allocator));
    if next.init != Some(NextCoderInitFunction::AutoDecoder(auto_decoder_init)) {
        lzma_next_end(next);
    }
    next.init = Some(NextCoderInitFunction::AutoDecoder(auto_decoder_init));

    if flags & !LZMA_SUPPORTED_FLAGS != 0 {
        return LzmaRet::OptionsError;
    }

    let mut coder: &mut LzmaAutoCoder = &mut LzmaAutoCoder::default();
    if next.coder.is_none() {
        let coder_ = LzmaAutoCoder {
            next: Box::new(LzmaNextCoder::default()),
            memlimit: my_max(1, memlimit),
            flags: flags,
            sequence: Sequence::SeqInit,
        };
        next.code = Some(auto_decode);
        next.end = Some(auto_decoder_end);
        next.get_check = Some(auto_decoder_get_check);
        next.memconfig = Some(auto_decoder_memconfig);
        next.coder = Some(CoderType::AutoDecoder(coder_));
    } else {
        coder = match next.coder {
            Some(CoderType::AutoDecoder(ref mut c)) => c,
            _ => return LzmaRet::ProgError,
        };
    }

    coder.memlimit = my_max(1, memlimit);
    coder.flags = flags;
    coder.sequence = Sequence::SeqInit;
    coder.next = Box::new(LzmaNextCoder::default());
    LzmaRet::Ok
}

pub fn lzma_auto_decoder(strm: &mut LzmaStream, memlimit: u64, flags: u32) -> LzmaRet {
    // lzma_next_strm_init(auto_decoder_init, strm, memlimit, flags);

    let ret_: LzmaRet = lzma_strm_init(Some(strm));
    if ret_ != LzmaRet::Ok {
        return ret_;
    }
    let ret_0: LzmaRet = auto_decoder_init(
        &mut strm
            .internal
            .borrow_mut()
            .as_mut()
            .unwrap()
            .next
            .as_mut()
            .unwrap(),
        memlimit,
        flags,
    );
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
