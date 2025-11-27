/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

 #[macro_use]
 use crate::api::{
     LzmaAction, LzmaOptionsLzma, LzmaOptionsType, LzmaRet, LzmaStream, LzmaVli,
     LZMA_FILTER_LZMA1EXT, LZMA_LZMA1EXT_ALLOW_EOPM, LZMA_VLI_UNKNOWN,
 };
 use crate::common::CoderType;
 use crate::{
     api::lzma_set_ext_size,
     common::{
         lzma_end, lzma_next_end, lzma_next_filter_init, lzma_strm_init, LzmaFilterInfo,
         LzmaNextCoder, NextCoderInitFunction, LZMA_MEMUSAGE_BASE,
     },
     lzma::{lzma_lzma_decoder_init, lzma_lzma_decoder_memusage, lzma_lzma_lclppb_decode},
 };
 
 use common::my_max;
 
 #[repr(i32)]
 #[derive(Clone, PartialEq, Debug, Default)]
 pub enum Sequence {
     #[default]
     Properties,
     DictionarySize,
     UncompressedSize,
     CoderInit,
     Code,
 }
 
 #[repr(C)]
 #[derive(Debug, Default)]
 pub struct LzmaAloneDecoder {
     pub next: Box<LzmaNextCoder>,
     pub sequence: Sequence,
     pub picky: bool,
     pub pos: usize,
     pub uncompressed_size: LzmaVli,
     pub memlimit: u64,
     pub memusage: u64,
     pub options: LzmaOptionsLzma,
 }
 
 fn alone_decode(
     coder_ptr: &mut CoderType,
 
     in_: &Vec<u8>,
     in_pos: &mut usize,
     in_size: usize,
     out: &mut [u8],
     out_pos: &mut usize,
     out_size: usize,
     action: LzmaAction,
 ) -> LzmaRet {
     // let coder = CoderType::AloneDecoder(coder_ptr);
     // let mut coder = match coder_ptr {
     //     CoderType::AloneDecoder(mut c) => c,
     //     _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
     // };
     if let CoderType::AloneDecoder(ref mut coder) = coder_ptr {
         while *out_pos < out_size && (coder.sequence == Sequence::Code || *in_pos < in_size) {
             match coder.sequence {
                 Sequence::Properties => {
                     if lzma_lzma_lclppb_decode(&mut coder.options, in_[*in_pos]) != false {
                         return LzmaRet::FormatError;
                     }
                     coder.sequence = Sequence::DictionarySize;
                     *in_pos += 1;
                 }
                 Sequence::DictionarySize => {
                     coder.options.dict_size |= (in_[*in_pos] as u32) << (coder.pos * 8);
                     if coder.pos == 3 {
                         if coder.picky && coder.options.dict_size != u32::MAX as u32 {
                             let mut d = coder.options.dict_size - 1;
                             d |= d >> 2;
                             d |= d >> 3;
                             d |= d >> 4;
                             d |= d >> 8;
                             d |= d >> 16;
                             d += 1;
                             if d != coder.options.dict_size {
                                 return LzmaRet::FormatError;
                             }
                         }
                         coder.pos = 0;
                         coder.sequence = Sequence::UncompressedSize;
                     } else {
                         coder.pos += 1;
                     }
                     *in_pos += 1;
                 }
                 Sequence::UncompressedSize => {
                     coder.uncompressed_size |= (in_[*in_pos] as LzmaVli) << (coder.pos * 8);
                     *in_pos += 1;
                     if coder.pos == 7 {
                         if coder.picky
                             && coder.uncompressed_size != LZMA_VLI_UNKNOWN
                             && coder.uncompressed_size >= (1u64 << 38)
                         {
                             return LzmaRet::FormatError;
                         }
                         coder.options.ext_flags = LZMA_LZMA1EXT_ALLOW_EOPM;
                         lzma_set_ext_size(&mut coder.options, coder.uncompressed_size);
                         coder.memusage = lzma_lzma_decoder_memusage(
                             &LzmaOptionsType::LzmaOptionsLzma(coder.options.clone()),
                         ) + LZMA_MEMUSAGE_BASE;
                         coder.pos = 0;
                         coder.sequence = Sequence::CoderInit;
                     } else {
                         coder.pos += 1;
                     }
                 }
                 Sequence::CoderInit => {
                     if coder.memusage > coder.memlimit {
                         return LzmaRet::MemlimitError;
                     }
 
                     let mut filters = [
                         LzmaFilterInfo {
                             id: LZMA_FILTER_LZMA1EXT,
                             init: Some(lzma_lzma_decoder_init),
                             options: Some(LzmaOptionsType::LzmaOptionsLzma(coder.options.clone())),
                         },
                         LzmaFilterInfo {
                             id: 0,
                             init: None,
                             options: None,
                         },
                     ];
                     let ret = lzma_next_filter_init(&mut coder.next, &filters);
                     if ret != LzmaRet::Ok {
                         return ret;
                     }
                     coder.sequence = Sequence::Code;
                 }
                 Sequence::Code => {
                     if let Some(code) = coder.next.code {
                         return code(
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
                 }
                 _ => return LzmaRet::ProgError,
             }
         }
         LzmaRet::Ok
     } else {
         LzmaRet::ProgError
     }
 }
 
 pub fn alone_decoder_end(coder_ptr: &mut CoderType) {
     let coder = match coder_ptr {
         CoderType::AloneDecoder(ref mut c) => c,
         _ => return, // 如果不是 AloneDecoder 类型，则返回错误
     };
     // 结束下一个编码器
     lzma_next_end(&mut coder.next);
 }
 
 fn alone_decoder_memconfig(
     coder_ptr: &mut CoderType,
     memusage: &mut u64,
     old_memlimit: &mut u64,
     new_memlimit: u64,
 ) -> LzmaRet {
     // let coder = coder_ptr;
     let coder = match coder_ptr {
         CoderType::AloneDecoder(ref mut c) => c,
         _ => {
             return LzmaRet::ProgError;
         } // 如果不是 AloneDecoder 类型，则返回错误
     };
 
     *memusage = coder.memusage;
     *old_memlimit = coder.memlimit;
 
     if new_memlimit != 0 {
         if new_memlimit < coder.memusage {
             return LzmaRet::MemlimitError;
         }
         coder.memlimit = new_memlimit;
     }
 
     LzmaRet::Ok
 }
 
 pub fn lzma_alone_decoder_init(next: &mut LzmaNextCoder, memlimit: u64, picky: bool) -> LzmaRet {
     if next.init != Some(NextCoderInitFunction::AloneDecoder(lzma_alone_decoder_init)) {
         lzma_next_end(next);
     }
     next.init = Some(NextCoderInitFunction::AloneDecoder(lzma_alone_decoder_init));
 
     let coder: &mut LzmaAloneDecoder;
     if next.coder.is_none() {
         let new_coder = LzmaAloneDecoder {
             next: Box::new(LzmaNextCoder::default()), // Use default without mutable reference
             sequence: Sequence::Properties,
             picky,
             pos: 0,
             uncompressed_size: 0,
             memlimit: my_max(1, memlimit),
             memusage: LZMA_MEMUSAGE_BASE,
             options: LzmaOptionsLzma::default(),
         };
         next.code = Some(alone_decode);
         next.end = Some(alone_decoder_end);
         next.memconfig = Some(alone_decoder_memconfig);
         next.coder = Some(CoderType::AloneDecoder(new_coder));
         // 这里需要重新获取 coder 引用
         if let Some(CoderType::AloneDecoder(ref mut c)) = next.coder {
             coder = c;
         } else {
             return LzmaRet::ProgError;
         }
     } else {
         coder = match next.coder.as_mut().unwrap() {
             CoderType::AloneDecoder(ref mut c) => c,
             _ => {
                 return LzmaRet::ProgError;
             } // 如果不是 AloneDecoder 类型，则返回错误
         };
     }
 
     coder.sequence = Sequence::Properties;
     coder.picky = picky;
     coder.pos = 0;
     coder.options.dict_size = 0;
     coder.options.preset_dict = None;
     coder.options.preset_dict_size = 0;
     coder.uncompressed_size = 0;
     coder.memlimit = my_max(1, memlimit);
     coder.memusage = LZMA_MEMUSAGE_BASE;
     coder.next = Box::new(LzmaNextCoder::default());
     LzmaRet::Ok
 }
 
 pub fn lzma_alone_decoder(strm: &mut LzmaStream, memlimit: u64) -> LzmaRet {
     let ret_: LzmaRet = lzma_strm_init(Some(strm));
     if ret_ != LzmaRet::Ok {
         return ret_;
     }
 
     let mut internal = strm.internal.borrow_mut();
     if let Some(ref mut internal) = *internal {
         if let Some(ref mut next) = internal.next {
             let ret_0: LzmaRet = lzma_alone_decoder_init(next, memlimit, false);
             if ret_0 != LzmaRet::Ok {
                 drop(internal);
                 // lzma_end(Some(strm));
                 return ret_0;
             }
 
             internal.supported_actions[LzmaAction::Run as usize] = true;
             internal.supported_actions[LzmaAction::Finish as usize] = true;
 
             LzmaRet::Ok
         } else {
             drop(internal);
             // lzma_end(Some(strm));
             LzmaRet::ProgError
         }
     } else {
         drop(internal);
         // lzma_end(Some(strm));
         LzmaRet::ProgError
     }
 }
 