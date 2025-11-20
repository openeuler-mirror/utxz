/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaAction, LzmaFilter, LzmaMatchFinder, LzmaRet},
    common::LzmaNextCoder,
    // lz::{
    //     lzma_mf_bt2_find, lzma_mf_bt2_skip, lzma_mf_bt3_find, lzma_mf_bt3_skip, lzma_mf_bt4_find,
    //     lzma_mf_bt4_skip, lzma_mf_hc3_find, lzma_mf_hc3_skip, lzma_mf_hc4_find, lzma_mf_hc4_skip,
    // },
    lzma::{LzmaLzma1Encoder, LzmaLzma2Encoder},
};

use super::lzma_mf_find;
pub const MF_FIND: fn(&mut LzmaMf, &mut u32, &mut [LzmaMatch]) -> u32 = lzma_mf_find;

#[derive(Debug, Clone, Default, Copy)]
pub struct LzmaMatch {
    pub len: u32,
    pub dist: u32,
}

#[derive(Debug, Clone)]
pub struct LzmaMf {
    pub buffer: Vec<u8>,
    pub size: u32,
    pub keep_size_before: u32,
    pub keep_size_after: u32,
    pub offset: u32,
    pub read_pos: u32,
    pub read_ahead: u32,
    pub read_limit: u32,
    pub write_pos: u32,
    pub pending: u32,
    pub find: Option<fn(mf: &mut LzmaMf, matches: &mut [LzmaMatch]) -> u32>,
    pub skip: Option<fn(mf: &mut LzmaMf, num: u32)>,
    pub hash: Vec<u32>,
    pub son: Vec<u32>,
    pub cyclic_pos: u32,
    pub cyclic_size: u32,
    pub hash_mask: u32,
    pub depth: u32,
    pub nice_len: u32,
    pub match_len_max: u32,
    pub action: LzmaAction,
    pub hash_count: u32,
    pub sons_count: u32,
}
impl<'a> Default for LzmaMf {
    fn default() -> Self {
        LzmaMf {
            buffer: Vec::new(),
            size: 0,
            keep_size_before: 0,
            keep_size_after: 0,
            offset: 0,
            read_pos: 0,
            read_ahead: 0,
            read_limit: 0,
            write_pos: 0,
            pending: 0,
            find: None,
            skip: None,
            hash: Vec::new(),
            son: Vec::new(),
            cyclic_pos: 0,
            cyclic_size: 0,
            hash_mask: 0,
            depth: 0,
            nice_len: 0,
            match_len_max: 0,
            action: LzmaAction::Run,
            hash_count: 0,
            sons_count: 0,
        }
    }
}

impl LzmaMf {
    // 返回数组索引位置
    pub fn mf_ptr(&self, index: u32) -> usize {
        (self.read_pos - index) as usize
    }

    pub fn cur_offset(&self) -> u32 {
        self.read_pos
    }
}
#[derive(Debug, Clone, Default)]
pub struct LzmaLzOptions {
    /// 保持在 "实际" 字典前面可用的额外数据量
    pub before_size: usize,

    /// 历史缓冲区的大小
    pub dict_size: usize,

    /// 保持在 "实际" 字典后面可用的额外数据量
    pub after_size: usize,

    /// LZ 编码器可以接受的最大匹配长度。
    /// 这用于将 nice_len 长度的匹配扩展到最大可能的长度。
    pub match_len_max: usize,

    /// 匹配查找器将搜索到此长度的匹配。
    /// 这个值必须小于或等于 match_len_max。
    pub nice_len: usize,

    /// 使用的匹配查找器类型
    pub match_finder: LzmaMatchFinder,

    /// 最大搜索深度
    pub depth: u32,

    /// 预设字典（TODO: 需要说明）
    pub preset_dict: Option<Vec<u8>>,

    /// 预设字典的大小
    pub preset_dict_size: u32,
}

#[derive(Debug)]
pub enum LzEncoderType {
    Lzma2Encoder(LzmaLzma2Encoder),
    LzmaEncoderPrivate(LzmaLzma1Encoder),
}
#[derive(Debug, Default)]
pub struct LzmaLzEncoder {
    pub coder: Option<LzEncoderType>,
    pub code: Option<
        fn(
            coder: &mut LzEncoderType,
            mf: &mut LzmaMf,
            out: &mut [u8],
            out_pos: &mut usize,
            out_size: usize,
        ) -> LzmaRet,
    >,
    pub end: Option<fn(coder: &mut LzEncoderType)>,
    pub options_update: Option<fn(coder: &mut LzEncoderType, filter: &LzmaFilter) -> LzmaRet>,
    pub set_out_limit:
        Option<fn(coder: &mut LzEncoderType, uncomp_size: &mut u64, out_limit: u64) -> LzmaRet>,
}

#[derive(Debug, Default)]
pub struct LzmaEncoder {
    pub lz: LzmaLzEncoder,
    pub mf: LzmaMf,
    pub next: Box<LzmaNextCoder>,
}

pub fn mf_ptr<'a>(mf: &'a LzmaMf) -> &[u8] {
    &mf.buffer[mf.read_pos as usize..]
}

pub fn mf_avail(mf: &LzmaMf) -> u32 {
    mf.write_pos - mf.read_pos
}