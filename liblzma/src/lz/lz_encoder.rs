/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::my_min;
use libc::memset;

use crate::{
    api::{
        LzmaAction, LzmaFilter, LzmaMatchFinder, LzmaOptionsType, LzmaRet, LzmaVli,
        LZMA_DICT_SIZE_MIN,
    },
    common::{
        lzma_bufcpy, lzma_next_end, lzma_next_filter_init, lzma_next_filter_update, CoderType,
        LzmaFilterInfo, LzmaNextCoder, LZMA_MEMCMPLEN_EXTRA,
    },
    lz::{
        lzma_mf_bt2_find, lzma_mf_bt2_skip, lzma_mf_bt3_find, lzma_mf_bt3_skip, lzma_mf_bt4_find,
        lzma_mf_bt4_skip, lzma_mf_hc3_find, lzma_mf_hc3_skip, lzma_mf_hc4_find, lzma_mf_hc4_skip,
    },
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
#[derive(Debug)]
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
impl Default for LzmaLzEncoder {
    fn default() -> Self {
        LzmaLzEncoder {
            coder: None,          // Option 类型默认是 None
            code: None,           // Option 类型默认是 None
            end: None,            // Option 类型默认是 None
            options_update: None, // Option 类型默认是 None
            set_out_limit: None,  // Option 类型默认是 None
        }
    }
}

#[derive(Debug, Default)]
pub struct LzmaEncoder {
    pub lz: LzmaLzEncoder,
    pub mf: LzmaMf,
    pub next: Box<LzmaNextCoder>,
}

pub fn mf_get_hash_bytes(match_finder: LzmaMatchFinder) -> u32 {
    match_finder as u32 & 0x0F
}

pub fn mf_ptr<'a>(mf: &'a LzmaMf) -> &[u8] {
    &mf.buffer[mf.read_pos as usize..]
}

pub fn mf_avail(mf: &LzmaMf) -> u32 {
    mf.write_pos - mf.read_pos
}

pub fn mf_unencoded(mf: &LzmaMf) -> u32 {
    mf.write_pos - mf.read_pos + mf.read_ahead
}

pub fn mf_position(mf: &LzmaMf) -> u32 {
    mf.read_pos - mf.read_ahead
}

pub fn mf_skip(mf: &mut LzmaMf, amount: u32) {
    if amount != 0 {
        (mf.skip.unwrap())(mf, amount);
        mf.read_ahead += amount;
    }
}

pub fn mf_read(
    mf: &mut LzmaMf,
    out: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    left: &mut usize,
) {
    let out_avail = out_size - *out_pos;
    let copy_size = out_avail.min(*left);

    // 断言检查
    assert!(mf.read_ahead == 0);
    assert!(mf.read_pos >= *left as u32);

    // 获取需要复制的数据切片
    let src = &mf.buffer[mf.read_pos as usize - *left..mf.read_pos as usize - *left + copy_size];

    // 将数据从切片复制到输出缓冲区
    // out.extend_from_slice(src);
    out[*out_pos..*out_pos + copy_size].copy_from_slice(src);

    // 更新输出位置和剩余字节数
    *out_pos += copy_size;
    *left -= copy_size;
}

fn move_window(mf: &mut LzmaMf) {
    // 对齐移动，使其对 16 字节对齐
    // 一些 LZ 编码器（如 LZMA）使用 mf.read_pos 的最低位来知道
    // 未压缩数据的对齐方式。对齐的缓冲区也能提升 memmove() 的速度。
    assert!(mf.read_pos > mf.keep_size_before);

    // 计算移动的偏移量
    let move_offset = (mf.read_pos - mf.keep_size_before) & !0xF;

    assert!(mf.write_pos > move_offset);
    let move_size = mf.write_pos - move_offset;

    assert!(move_offset + move_size <= mf.size);

    // 将数据从原位置移动到目标位置
    mf.buffer.copy_within(
        move_offset as usize..move_offset as usize + (move_size as usize),
        0,
    );

    // 更新状态
    mf.offset += move_offset;
    mf.read_pos -= move_offset;
    mf.read_limit -= move_offset;
    mf.write_pos -= move_offset;
}

fn fill_window(
    coder: &mut LzmaEncoder,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    assert!(coder.mf.read_pos <= coder.mf.write_pos);

    // 如果需要，移动滑动窗口
    if coder.mf.read_pos >= coder.mf.size - coder.mf.keep_size_after {
        move_window(&mut coder.mf);
    }

    let mut write_pos: usize = coder.mf.write_pos as usize;
    let mut ret: LzmaRet = LzmaRet::Ok;

    if coder.next.code.is_none() {
        // 不使用过滤器，直接复制数据
        lzma_bufcpy(
            input,
            in_pos,
            in_size,
            &mut coder.mf.buffer,
            &mut write_pos,
            coder.mf.size as usize,
        );

        ret = if action.clone() != LzmaAction::Run && *in_pos == in_size {
            LzmaRet::StreamEnd
        } else {
            LzmaRet::Ok
        };
    } else {
        if let Some(code) = coder.next.code {
            ret = code(
                &mut coder.next.coder.as_mut().unwrap(),
                input,
                in_pos,
                in_size,
                &mut coder.mf.buffer,
                &mut write_pos,
                coder.mf.size as usize,
                action.clone(),
            );
        }
    }

    coder.mf.write_pos = write_pos as u32;

    // 清零未使用的缓冲区部分
    coder.mf.buffer[write_pos..write_pos + LZMA_MEMCMPLEN_EXTRA].fill(0);

    if ret == LzmaRet::StreamEnd {
        assert!(*in_pos == in_size);
        ret = LzmaRet::Ok;
        coder.mf.action = action.clone();
        coder.mf.read_limit = coder.mf.write_pos;
    } else if coder.mf.write_pos > coder.mf.keep_size_after {
        coder.mf.read_limit = coder.mf.write_pos - coder.mf.keep_size_after;
    }

    if coder.mf.pending > 0 && coder.mf.read_pos < coder.mf.read_limit {
        let pending = coder.mf.pending;
        coder.mf.pending = 0;

        assert!(coder.mf.read_pos >= pending);
        coder.mf.read_pos -= pending;

        coder.mf.skip.unwrap()(&mut coder.mf, pending);
    }

    ret
}

fn lz_encode(
    coder_ptr: &mut CoderType,
    input: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
    output: &mut [u8],
    out_pos: &mut usize,
    out_size: usize,
    action: LzmaAction,
) -> LzmaRet {
    // let coder = coder_ptr;
    // println!("============ lz_encode");
    let coder = match coder_ptr {
        CoderType::LzEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    while *out_pos < out_size && (*in_pos < in_size || action != LzmaAction::Run) {
        // 如果需要，从输入读取更多数据到 coder->mf.buffer
        if coder.mf.action == LzmaAction::Run && coder.mf.read_pos >= coder.mf.read_limit {
            let ret = fill_window(coder, input, in_pos, in_size, action.clone());
            if ret != LzmaRet::Ok {
                return ret;
            }
        }

        // 编码
        let mut ret: LzmaRet = LzmaRet::Ok;
        if let Some(code) = coder.lz.code {
            ret = code(
                &mut coder.lz.coder.as_mut().unwrap(),
                &mut coder.mf,
                output,
                out_pos,
                out_size,
            );
        }

        if ret != LzmaRet::Ok {
            // 在刷新时将其设置为 LZMA_RUN
            coder.mf.action = LzmaAction::Run;
            return ret;
        }
    }

    LzmaRet::Ok
}

const HASH_2_SIZE: usize = 1 << 10;
const HASH_3_SIZE: usize = 1 << 16;
fn lz_encoder_prepare(mf: &mut LzmaMf, lz_options: &LzmaLzOptions) -> bool {
    // 字典大小限制为 1.5 GiB
    if lz_options.dict_size < LZMA_DICT_SIZE_MIN as usize
        || lz_options.dict_size > ((1u32 << 30) + (1u32 << 29)) as usize
        || lz_options.nice_len > lz_options.match_len_max
    {
        return true;
    }

    mf.keep_size_before = (lz_options.before_size + lz_options.dict_size) as u32;
    mf.keep_size_after = (lz_options.after_size + lz_options.match_len_max) as u32;

    // 分配额外空间以避免频繁的 memmove()
    let mut reserve = lz_options.dict_size / 2;
    if reserve > (1u32 << 30) as usize {
        reserve /= 2;
    }

    reserve += (lz_options.before_size + lz_options.match_len_max + lz_options.after_size) / 2
        + (1u32 << 19) as usize;

    let old_size = mf.size;
    mf.size = mf.keep_size_before + reserve as u32 + mf.keep_size_after;

    // 如果旧的缓冲区存在且大小不同，则释放旧的缓冲区
    if !mf.buffer.is_empty() && old_size != mf.size {
        mf.buffer.clear();
    }

    // 匹配查找器选项
    mf.match_len_max = lz_options.match_len_max as u32;
    mf.nice_len = lz_options.nice_len as u32;

    mf.cyclic_size = lz_options.dict_size as u32 + 1;

    // 验证匹配查找器 ID 并设置函数指针
    match lz_options.match_finder {
        LzmaMatchFinder::LzmaMfHc3 => {
            mf.find = Some(lzma_mf_hc3_find);
            mf.skip = Some(lzma_mf_hc3_skip);
        }

        LzmaMatchFinder::LzmaMfHc4 => {
            mf.find = Some(lzma_mf_hc4_find);
            mf.skip = Some(lzma_mf_hc4_skip);
        }

        LzmaMatchFinder::LzmaMfBt2 => {
            mf.find = Some(lzma_mf_bt2_find);
            mf.skip = Some(lzma_mf_bt2_skip);
        }

        LzmaMatchFinder::LzmaMfBt3 => {
            mf.find = Some(lzma_mf_bt3_find);
            mf.skip = Some(lzma_mf_bt3_skip);
        }

        LzmaMatchFinder::LzmaMfBt4 => {
            mf.find = Some(lzma_mf_bt4_find);
            mf.skip = Some(lzma_mf_bt4_skip);
        }
        _ => {
            return true;
        }
    }

    // 计算 mf.hash 和 mf.son 的大小
    let hash_bytes = mf_get_hash_bytes(lz_options.match_finder.clone());
    assert!(hash_bytes <= mf.nice_len);

    let is_bt = (lz_options.match_finder.clone() as u32 & 0x10) != 0;
    let mut hs;

    if hash_bytes == 2 {
        hs = 0xFFFF;
    } else {
        hs = lz_options.dict_size - 1;
        hs |= hs >> 1;
        hs |= hs >> 2;
        hs |= hs >> 4;
        hs |= hs >> 8;
        hs >>= 1;
        hs |= 0xFFFF;

        if hs > (1 << 24) {
            if hash_bytes == 3 {
                hs = (1 << 24) - 1;
            } else {
                hs >>= 1;
            }
        }
    }

    mf.hash_mask = hs as u32;

    hs += 1;
    if hash_bytes > 2 {
        hs += HASH_2_SIZE;
    }
    if hash_bytes > 3 {
        hs += HASH_3_SIZE;
    }

    let old_hash_count = mf.hash_count;
    let old_sons_count = mf.sons_count;
    mf.hash_count = hs as u32;
    mf.sons_count = mf.cyclic_size;
    if is_bt {
        mf.sons_count *= 2;
    }

    // 如果旧的哈希数组存在且大小不同，则释放旧的哈希数组
    if old_hash_count != mf.hash_count || old_sons_count != mf.sons_count {
        mf.hash.clear();

        mf.son.clear();
    }

    // 最大匹配查找器循环次数
    mf.depth = lz_options.depth;
    if mf.depth == 0 {
        if is_bt {
            mf.depth = 16 + mf.nice_len / 2;
        } else {
            mf.depth = 4 + mf.nice_len / 4;
        }
    }

    false
}

fn lz_encoder_init(mf: &mut LzmaMf, lz_options: &LzmaLzOptions) -> bool {
    // 分配历史缓冲区
    if mf.buffer.is_empty() {
        // 初始化额外字节
        mf.buffer = vec![0u8; mf.size as usize + LZMA_MEMCMPLEN_EXTRA];
    }

    // 使用 cyclic_size 作为初始 mf.offset
    mf.offset = mf.cyclic_size;
    mf.read_pos = 0;
    mf.read_ahead = 0;
    mf.read_limit = 0;
    mf.write_pos = 0;
    mf.pending = 0;

    // 分配并初始化哈希表
    if mf.hash.is_empty() {
        // 先关闭内存申请
        mf.hash = vec![0u32; mf.hash_count as usize];
        mf.son = vec![0u32; mf.sons_count as usize];

        if mf.hash.is_empty() || mf.son.is_empty() {
            mf.hash.clear();
            mf.son.clear();

            return true;
        }
    } else {
        mf.hash.fill(0);
    }

    mf.cyclic_pos = 0;

    // 处理预设字典
    if let Some(preset_dict) = &lz_options.preset_dict {
        if lz_options.preset_dict_size > 0 {
            mf.write_pos = std::cmp::min(lz_options.preset_dict_size, mf.size);
            mf.buffer[..mf.write_pos as usize].copy_from_slice(
                &preset_dict[(lz_options.preset_dict_size - mf.write_pos) as usize..],
            );
            mf.action = LzmaAction::SyncFlush;
            mf.skip.unwrap()(mf, mf.write_pos);
        }
    }

    mf.action = LzmaAction::Run;

    false
}

pub fn lzma_lz_encoder_memusage(lz_options: &LzmaLzOptions) -> u64 {
    let mut mf = LzmaMf {
        buffer: Vec::new(),
        hash: Vec::new(),
        son: Vec::new(),
        hash_count: 0,
        sons_count: 0,
        ..Default::default()
    };

    if lz_encoder_prepare(&mut mf, lz_options) {
        return u64::MAX;
    }

    ((mf.hash_count as u64) + (mf.sons_count as u64)) * std::mem::size_of::<u32>() as u64
        + mf.size as u64
        + std::mem::size_of::<LzmaEncoder>() as u64
}

pub fn lz_encoder_end(coder_ptr: &mut CoderType) {
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::LzEncoder(ref mut c) => c,
        _ => return, // 如果不是 AloneDecoder 类型，则返回错误
    };
    // 释放历史缓冲区
    coder.mf.buffer.clear();
    coder.mf.hash.clear();
    coder.mf.son.clear();

    // 结束下一个编码器
    lzma_next_end(&mut coder.next);
    if let Some(end_fn) = coder.lz.end {
        end_fn(&mut coder.lz.coder.as_mut().unwrap());
    } else {
        coder.lz.coder = None;
    }
}

pub fn lz_encoder_update(
    coder_ptr: &mut CoderType,

    _filters_null: Option<&[LzmaFilter]>,
    reversed_filters: &[LzmaFilter],
) -> LzmaRet {
    // 将 coder_ptr 转换为具体的 LzmaEncoder 类型
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::LzEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    if coder.lz.options_update.is_none() {
        return LzmaRet::ProgError;
    }

    // 调用 options_update 函数
    let mut ret: LzmaRet = LzmaRet::Ok;
    if let Some(options_update) = coder.lz.options_update {
        ret = options_update(&mut coder.lz.coder.as_mut().unwrap(), &reversed_filters[0]);
    }
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 更新下一个过滤器
    lzma_next_filter_update(&mut coder.next, &reversed_filters[1..])
}

fn lz_encoder_set_out_limit(
    coder_ptr: &mut CoderType,
    uncomp_size: &mut u64,
    out_limit: u64,
) -> LzmaRet {
    // let coder = coder_ptr;
    let coder = match coder_ptr {
        CoderType::LzEncoder(ref mut c) => c,
        _ => return LzmaRet::ProgError, // 如果不是 AloneDecoder 类型，则返回错误
    };

    if coder.next.code.is_none() && coder.lz.set_out_limit.is_some() {
        if let Some(set_out_limit) = coder.lz.set_out_limit {
            return set_out_limit(coder.lz.coder.as_mut().unwrap(), uncomp_size, out_limit);
        }
    }

    LzmaRet::OptionsError
}

pub fn lzma_lz_encoder_init(
    next: &mut LzmaNextCoder,
    filters: &[LzmaFilterInfo],
    lz_init: fn(&mut LzmaLzEncoder, LzmaVli, &LzmaOptionsType, &mut LzmaLzOptions) -> LzmaRet,
) -> LzmaRet {
    let mut coder: &mut LzmaEncoder = &mut LzmaEncoder::default();

    if next.coder.is_none() {
        let coder_ = LzmaEncoder::default();
        // 初始化 next 字段
        next.coder = Some(CoderType::LzEncoder(coder_));
        next.code = Some(lz_encode);
        next.end = Some(lz_encoder_end);
        next.update = Some(lz_encoder_update);
        next.set_out_limit = Some(lz_encoder_set_out_limit);
    }

    coder = match next.coder {
        Some(CoderType::LzEncoder(ref mut c)) => c,
        _ => return LzmaRet::ProgError,
    };

    // 初始化 LZ 编码器
    let mut lz_options = LzmaLzOptions::default();

    let ret = lz_init(
        &mut coder.lz,
        filters[0].id,
        &mut filters[0].options.clone().unwrap(),
        &mut lz_options,
    );
    if ret != LzmaRet::Ok {
        return ret;
    }

    // 设置 coder->mf 的大小信息，并释放旧的缓冲区（如果大小不匹配）
    if (lz_encoder_prepare(&mut coder.mf, &lz_options)) {
        return LzmaRet::OptionsError;
    }

    // 如果需要，分配新的缓冲区，并完成初始化
    if (lz_encoder_init(&mut coder.mf, &lz_options)) {
        return LzmaRet::MemError;
    }

    // 初始化链中的下一个过滤器（如果有的话）
    lzma_next_filter_init(&mut coder.next, &filters[1..])
}

pub fn lzma_mf_is_supported(mf: LzmaMatchFinder) -> bool {
    match mf {
        _ => false,
    }
}
