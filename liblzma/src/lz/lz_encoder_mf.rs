/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::LzmaAction,
    check::CRC32_TABLE,
    common::{lzma_memcmplen, lzma_memcmplen_unchecked},
    lz::mf_ptr,
};

use super::{mf_avail, LzmaMatch, LzmaMf};

const MUST_NORMALIZE_POS: u32 = u32::MAX;
const EMPTY_HASH_VALUE: u32 = 0;
pub fn lzma_mf_find(mf: &mut LzmaMf, count_ptr: &mut u32, matches: &mut [LzmaMatch]) -> u32 {
    // 调用匹配查找器，返回找到的长度-距离对的数量
    // 这个地方调用的lzma_mf_bt4_find函数
    let count = (mf.find.unwrap())(mf, matches);
    // println!("count = {}", count);
    // 假设没有找到匹配项，因此最长匹配长度为零
    let mut len_best = 0;
    // println!("matches[i].dist {}", matches[0].dist);
    // println!("matches[0].dist {:#?}", matches[0]);
    if count > 0 {
        // 验证匹配项
        for i in 0..count as usize {
            assert!(matches[i].len <= mf.nice_len);
            assert!(matches[i].dist < mf.read_pos);

            assert_eq!(
                // mf_ptr(mf)[..matches[i].len as usize],
                // mf_ptr(mf)[..matches[i].dist as usize]
                mf.buffer[(mf.read_pos - 1) as usize],
                mf.buffer[(mf.read_pos - matches[i].dist - 2) as usize]
            );
        }

        // 数组中最后一个元素包含最长的匹配项
        len_best = matches[count as usize - 1].len;
        // println!("**** len_best {}, mf.nice_len = {} ********", len_best, mf.nice_len);
        // 如果找到了最大搜索长度的匹配项，尝试将匹配项扩展到最大可能长度
        if len_best == mf.nice_len {
            // 匹配长度的限制是 LZ 编码器支���的最大匹配长度或字典中剩余的字节数，以较小者为准
            let mut limit = mf_avail(mf) + 1;
            if limit > mf.match_len_max {
                limit = mf.match_len_max;
            }

            // 指向刚刚通过匹配查找器的字节的指针
            let p1 = &mf.buffer[mf.mf_ptr(1)..];

            // 指向匹配开始位置的指针
            let p2 =
                &mf.buffer[mf.mf_ptr(1) as usize - matches[count as usize - 1].dist as usize - 1..];

            len_best = lzma_memcmplen(p1, p2, len_best, limit);
        }
    }

    *count_ptr = count;

    // 更新读取位置以指示匹配查找器已为此字典偏移量运行
    mf.read_ahead += 1;

    len_best
}

fn normalize(mf: &mut LzmaMf) {
    assert!(mf.read_pos + mf.offset == MUST_NORMALIZE_POS);

    let subvalue = MUST_NORMALIZE_POS - mf.cyclic_size;

    for i in 0..mf.hash_count {
        if mf.hash[i as usize] <= subvalue {
            mf.hash[i as usize] = EMPTY_HASH_VALUE;
        } else {
            mf.hash[i as usize] -= subvalue;
        }
    }

    for i in 0..mf.sons_count {
        if mf.son[i as usize] <= subvalue {
            mf.son[i as usize] = EMPTY_HASH_VALUE;
        } else {
            mf.son[i as usize] -= subvalue;
        }
    }

    mf.offset -= subvalue;
}

fn move_pos(mf: &mut LzmaMf) {
    mf.cyclic_pos = mf.cyclic_pos.wrapping_add(1);
    if mf.cyclic_pos == mf.cyclic_size {
        mf.cyclic_pos = 0; // Wrap around to start
    }

    mf.read_pos += 1;
    assert!(mf.read_pos <= mf.write_pos);

    if mf.read_pos + mf.offset == u32::MAX {
        normalize(mf);
    }
}

fn move_pending(mf: &mut LzmaMf) {
    mf.read_pos += 1;
    assert!(mf.read_pos <= mf.write_pos);
    mf.pending += 1;
}
/// 根据 HC3 算法查找匹配项
///
/// 参数说明：
/// - `len_limit`：匹配长度上限
/// - `pos`：当前数据在整个缓冲区中的位置（绝对位置）
/// - `buf`：整个数据缓冲区（包含当前数据段以及历史数据）
/// - `cur_index`：当前数据段在 buf 中的起始索引
/// - `cur_match`：当前匹配候选位置（绝对位置）
/// - `depth`：查找深度
/// - `son`：辅助数组，用于存储匹配候选信息，长度应足够
/// - `cyclic_pos`：当前循环缓冲区位置
/// - `cyclic_size`：循环缓冲区总大小
/// - `matches`：存储匹配结果的数组切片，调用者保证容量足够
/// - `len_best`：当前已找到的最佳匹配长度
///
/// 返回值：匹配结果数组中匹配项的数量
pub fn hc_find_func(
    len_limit: u32,
    pos: u32,
    buf: &[u8],
    cur_index: usize,
    mut cur_match: u32,
    mut depth: u32,
    son: &mut [u32],
    cyclic_pos: u32,
    cyclic_size: u32,
    matches: &mut [LzmaMatch],
    mut len_best: u32,
) -> usize {
    // 将当前匹配候选值写入辅助数组的当前位置
    son[cyclic_pos as usize] = cur_match;
    // 初始化匹配数量为 0
    let mut match_count = 0;

    // 循环查找匹配
    loop {
        // 计算偏移量：当前位置与匹配候选位置之差
        let delta = pos - cur_match;
        // 如果查找深度用尽或 delta 超出循环缓冲区大小，则返回当前匹配数量
        if depth == 0 || delta >= cyclic_size {
            return match_count;
        }
        // 每次迭代减少查找深度
        depth -= 1;

        // 计算 pb 指针对应在缓冲区中的索引：当前数据起始位置向前偏移 delta
        let pb_index = match cur_index.checked_sub(delta as usize) {
            Some(idx) => idx,
            None => return match_count,
        };

        // 根据 C 代码更新 cur_match 值：
        // cur_match = son[cyclic_pos - delta + (if delta > cyclic_pos { cyclic_size } else { 0 })];
        let son_index = if delta > cyclic_pos {
            cyclic_pos as usize + cyclic_size as usize - delta as usize
        } else {
            cyclic_pos as usize - delta as usize
        };
        cur_match = son[son_index];

        // 判断：如果 pb[len_best] 与 cur[len_best]相等，且 pb[0] 与 cur[0]也相等，则进入匹配长度计算
        // pb[len_best] 对应 buf[pb_index + len_best]，cur[len_best] 对应 buf[cur_index + len_best]
        if ((pb_index + len_best as usize) < buf.len())
            && ((cur_index + len_best as usize) < buf.len())
            && (buf[pb_index + len_best as usize] == buf[cur_index + len_best as usize])
            && (buf[pb_index] == buf[cur_index])
        {
            // 根据安全切片调用 lzma_memcmplen 计算匹配长度，输入起始为 pb 和 cur
            let pb_slice = &buf[pb_index..];
            let cur_slice = &buf[cur_index..];
            let len = lzma_memcmplen(pb_slice, cur_slice, 1, len_limit);
            if len_best < len {
                len_best = len;
                // 保存匹配结果到 matches 数组中
                if match_count < matches.len() {
                    matches[match_count].len = len;
                    matches[match_count].dist = delta.wrapping_sub(1);
                    match_count += 1;
                }
                // 如果匹配长度已达到上限，则直接返回
                if len == len_limit {
                    return match_count;
                }
            }
        }
    }
}

/// 根据 HC3 算法查找匹配项
/// 返回找到的匹配数量
pub fn lzma_mf_hc3_find(mf: &mut LzmaMf, matches: &mut [LzmaMatch]) -> u32 {
    // 获取可用字节数作为匹配长度上限
    let mut len_limit = mf_avail(mf);
    if mf.nice_len <= len_limit {
        len_limit = mf.nice_len;
    } else if len_limit < 3 {
        // 当可用字节不足 3 时，调用 move_pending 更新状态，并返回 0
        move_pending(mf);
        return 0;
    }

    // 获取当前数据段（应保证返回一个切片，不使用 unsafe）
    let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
    let pos = mf.read_pos + mf.offset;
    let mut matches_count: u32 = 0;
    // 计算哈希值：temp = crc32_table[0][cur[0]] XOR cur[1]
    let temp = CRC32_TABLE[cur[0] as usize] ^ cur[1] as u32;
    let hash_2_value = temp & ((1 << 10) - 1);
    let hash_value = (temp ^ ((cur[2] as u32) << 8)) & mf.hash_mask;
    let delta2 = pos - mf.hash[hash_2_value as usize];
    let cur_match = mf.hash[(1 << 10) + hash_value as usize];
    // 更新全局哈希表
    mf.hash[hash_2_value as usize] = pos;
    mf.hash[(1 << 10) + hash_value as usize] = pos;
    let mut len_best: u32 = 2;
    // 判断前面位置是否有匹配：若 delta2 在循环区间内，并且前一个字节与当前字节相同
    if delta2 < mf.cyclic_size {
        // 使用安全索引方式访问 buffer，假定 mf.buffer 为完整数据缓冲区，
        // mf.cur_offset() 返回当前指针在 buffer 中的偏移
        let cur_index = mf.read_pos as usize;
        if cur_index >= delta2 as usize && mf.buffer[cur_index - delta2 as usize] == cur[0] {
            // 计算匹配长度（不超过 len_limit），lzma_memcmplen 已实现安全匹配长度计算
            len_best = lzma_memcmplen(
                &mf.buffer[(cur_index - delta2 as usize)..],
                cur,
                len_best,
                len_limit,
            );
            matches[0].len = len_best;
            // 距离为 delta2 - 1
            matches[0].dist = delta2.wrapping_sub(1);
            matches_count = 1;
            // 如果匹配长度达到了上限，则更新 son 数组（全局变量直接操作）并更新位置后返回
            if len_best == len_limit {
                mf.son[mf.cyclic_pos as usize] = cur_match;
                move_pos(mf);
                return 1;
            }
        }
    }

    // 调用查找函数，进一步更新匹配记录
    // hc_find_func 返回匹配记录数组尾指针与 matches 数组起始指针的偏移量（即匹配数）
    matches_count = hc_find_func(
        len_limit,
        pos,
        &mf.buffer,
        mf.read_pos as usize,
        cur_match,
        mf.depth,
        &mut mf.son,
        mf.cyclic_pos,
        mf.cyclic_size,
        &mut matches[matches_count as usize..],
        len_best,
    ) as u32;
    move_pos(mf);
    matches_count
}

/// 跳过 HC3 匹配查找过程中的指定数量
pub fn lzma_mf_hc3_skip(mf: &mut LzmaMf, mut amount: u32) {
    while amount != 0 {
        // 如果可用字节数不足 3，则调用 move_pending 更新状态后继续循环
        if mf_avail(mf) < 3 {
            move_pending(mf);
            continue;
        }
        // 获取当前数据切片
        let cur: &[u8] = mf_ptr(mf);
        let pos = mf.read_pos + mf.offset;
        let temp = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
        let hash_2_value = temp & ((1 << 10) - 1);
        let hash_value = (temp ^ ((cur[2] as u32) << 8)) & mf.hash_mask;
        let cur_match = mf.hash[(1 << 10) + hash_value as usize];
        // 更新全局哈希表
        mf.hash[hash_2_value as usize] = pos;
        mf.hash[(1 << 10) + hash_value as usize] = pos;
        // 更新 son 数组并移动位置
        mf.son[mf.cyclic_pos as usize] = cur_match;
        move_pos(mf);
        amount -= 1;
    }
}

/// 根据 HC4 算法查找匹配项
///
/// 参数说明：
/// - `len_limit`：匹配长度上限
/// - `pos`：当前数据在整个缓冲区中的位置（绝对位置）
/// - `buf`：整个数据缓冲区（包含当前数据段以及历史数据）
/// - `cur_index`：当前数据段在 buf 中的起始索引
/// - `cur_match`：当前匹配候选位置（绝对位置）
/// - `depth`：查找深度
/// - `son`：辅助数组，用于存储匹配候选信息，长度应足够
/// - `cyclic_pos`：当前循环缓冲区位置
/// - `cyclic_size`：循环缓冲区总大小
/// - `matches`：存储匹配结果的数组切片，调用者保证容量足够
/// - `len_best`：当前已找到的最佳匹配长度
///
/// 返回值：匹配结果数组中匹配项的数量
pub fn lzma_mf_hc4_find(mf: &mut LzmaMf, matches: &mut [LzmaMatch]) -> u32 {
    // 取可用字节数作为匹配长度上限
    let mut len_limit = mf_avail(mf);
    if mf.nice_len <= len_limit {
        len_limit = mf.nice_len;
    } else if len_limit < 4 {
        // 当可用字节不足 4 时，更新状态后返回 0
        move_pending(mf);
        return 0;
    }
    // 获取当前数据段，返回安全切片，不使用 unsafe
    let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
    let pos: u32 = mf.read_pos + mf.offset;
    let mut matches_count: u32 = 0;

    // 计算哈希值：temp = CRC32_TABLE[cur[0]] XOR cur[1]
    let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
    // 计算哈希相关值
    let hash_2_value: u32 = temp & ((1 << 10) - 1);
    let hash_3_value: u32 = (temp ^ ((cur[2] as u32) << 8)) & ((1 << 16) - 1);
    let hash_value: u32 =
        (temp ^ ((cur[2] as u32) << 8) ^ (CRC32_TABLE[cur[3] as usize] << 5)) & mf.hash_mask;

    // 计算 delta2 与 delta3，及当前匹配候选值
    // 注意：数组索引使用 usize 类型
    let mut delta2: u32 = pos - mf.hash[hash_2_value as usize];
    let delta3: u32 = pos - mf.hash[((1 << 10) as usize + hash_3_value as usize)];
    let cur_match: u32 = mf.hash[((1 << 10) as usize + (1 << 16) as usize + hash_value as usize)];
    // 更新全局哈希表
    mf.hash[hash_2_value as usize] = pos;
    mf.hash[((1 << 10) as usize + hash_3_value as usize)] = pos;
    mf.hash[((1 << 10) as usize + (1 << 16) as usize + hash_value as usize)] = pos;
    let mut len_best: u32 = 1;

    // 获取当前数据段在整体缓冲区中的起始索引（安全返回）
    let cur_index: usize = mf.cur_offset() as usize;

    // 判断前面位置是否有匹配：
    // 如果 delta2 小于循环缓冲区大小，且当前缓冲区中位于 (cur_index - delta2) 的字节与位于 cur_index 的相同
    if delta2 < mf.cyclic_size
        && cur_index >= delta2 as usize
        && mf.buffer[cur_index - delta2 as usize] == mf.buffer[cur_index]
    {
        len_best = 2;
        if let Some(first_match) = matches.get_mut(0) {
            first_match.len = 2;
            first_match.dist = delta2.wrapping_sub(1);
        }
        matches_count = 1;
    }
    // 如果 delta2 与 delta3 不相等，且 delta3 在循环区内，并且 buf[cur_index - delta3] 与 buf[cur_index]相同，
    // 则更新最佳匹配长度为 3，并记录匹配结果；同时更新 delta2 为 delta3
    if delta2 != delta3
        && delta3 < mf.cyclic_size
        && cur_index >= delta3 as usize
        && mf.buffer[cur_index - delta3 as usize] == mf.buffer[cur_index]
    {
        len_best = 3;
        if let Some(m) = matches.get_mut(matches_count as usize) {
            m.dist = delta3.wrapping_sub(1);
        }
        matches_count += 1;
        delta2 = delta3;
    }
    // 如果已找到至少一个匹配，则计算匹配长度更新最佳匹配长度
    if matches_count != 0 {
        let new_len = lzma_memcmplen(
            &mf.buffer[(cur_index - delta2 as usize)..],
            &mf.buffer[cur_index..],
            len_best,
            len_limit,
        );
        len_best = new_len;
        if let Some(last_match) = matches.get_mut((matches_count - 1) as usize) {
            last_match.len = len_best;
        }
        // 如果匹配长度达到上限，则更新辅助数组并移动查找器位置后返回匹配数
        if len_best == len_limit {
            mf.son[mf.cyclic_pos as usize] = cur_match;
            move_pos(mf);
            return matches_count;
        }
    }
    if len_best < 3 {
        len_best = 3;
    }
    // 调用 hc_find_func 查找更多匹配，假定该函数返回在提供的 matches 切片中写入的匹配数量
    let additional: usize = hc_find_func(
        len_limit,
        pos,
        cur,
        cur_index,
        cur_match,
        mf.depth,
        &mut mf.son,
        mf.cyclic_pos,
        mf.cyclic_size,
        &mut matches[matches_count as usize..],
        len_best,
    );
    matches_count += additional as u32;
    move_pos(mf);
    matches_count
}

/// 根据 HC4 算法跳过匹配查找过程中的指定数量
///
/// 参数说明：
/// - `mf`: 匹配查找器对象
/// - `amount`: 需要跳过的匹配数
pub fn lzma_mf_hc4_skip(mf: &mut LzmaMf, mut amount: u32) {
    // 循环跳过指定数量
    while amount != 0 {
        if mf_avail(mf) < 4 {
            move_pending(mf);
            amount -= 1;
            continue;
        }
        // 获取当前数据段切片
        let cur: &[u8] = mf_ptr(mf);
        let pos: u32 = mf.read_pos + mf.offset;
        let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
        let hash_2_value: u32 = temp & ((1 << 10) - 1);
        let hash_3_value: u32 = (temp ^ ((cur[2] as u32) << 8)) & ((1 << 16) - 1);
        let hash_value: u32 =
            (temp ^ ((cur[2] as u32) << 8) ^ (CRC32_TABLE[cur[3] as usize] << 5)) & mf.hash_mask;
        // 获取当前匹配候选值
        let cur_match: u32 = mf.hash[(1 << 10) as usize + (1 << 16) as usize + hash_value as usize];
        // 更新全局哈希表
        mf.hash[hash_2_value as usize] = pos;
        mf.hash[(1 << 10) as usize + hash_3_value as usize] = pos;
        mf.hash[(1 << 10) as usize + (1 << 16) as usize + hash_value as usize] = pos;
        // 更新辅助数组并移动查找器位置
        mf.son[mf.cyclic_pos as usize] = cur_match;
        move_pos(mf);
        amount -= 1;
    }
}

/// 根据 BT 算法查找匹配项
///
/// 参数说明：
/// - `len_limit`：匹配长度上限
/// - `pos`：当前数据在整个缓冲区中的位置（绝对位置）
/// - `cur`：整个数据缓冲区（包含当前数据段以及历史数据）
/// - `cur_match`：当前匹配候选位置（绝对位置）
/// - `depth`：查找深度
/// - `son`：辅助数组，用于存储匹配候选信息，长度应足够
/// - `cyclic_pos`：当前循环缓冲区位置
/// - `cyclic_size`：循环缓冲区总大小
/// - `matches`：存储匹配结果的数组切片，调用者保证容量足够
/// - `len_best`：当前已找到的最佳匹配长度
///
/// 返回值：匹配结果数组中匹配项的数量
pub fn bt_find_func(
    len_limit: u32,
    pos: u32,
    mf: &mut LzmaMf,
    mut cur_match: u32,
    matches: &mut [LzmaMatch],
    mut len_best: u32,
) -> usize {
    let mut depth = mf.depth;
    let cyclic_pos = mf.cyclic_pos;
    let cyclic_size = mf.cyclic_size;
    // ptr0 = son + (cyclic_pos << 1) + 1; ptr1 = son + (cyclic_pos << 1);
    let mut ptr0 = (cyclic_pos as usize) * 2 + 1;
    let mut ptr1 = (cyclic_pos as usize) * 2;
    let mut len0: u32 = 0;
    let mut len1: u32 = 0;
    let mut match_count: usize = 0;

    // SAFETY: buffer has mf.size + LZMA_MEMCMPLEN_EXTRA allocated bytes.
    // son has sons_count = cyclic_size * 2 entries for BT mode.
    // read_pos >= delta (checked below), and read_pos + len_limit + 8 < buffer.len()
    // due to LZMA_MEMCMPLEN_EXTRA padding. All pointer arithmetic is within bounds.
    let buf_ptr = mf.buffer.as_ptr();
    let son_ptr = mf.son.as_mut_ptr();

    loop {
        let delta = pos - cur_match;
        if depth == 0 || delta >= cyclic_size {
            // SAFETY: ptr0, ptr1 < cyclic_size * 2 = sons_count
            unsafe {
                *son_ptr.add(ptr0) = 0;
                *son_ptr.add(ptr1) = 0;
            }
            return match_count;
        }
        depth -= 1;

        // pair = son + ((cyclic_pos - delta + (if delta > cyclic_pos { cyclic_size } else { 0 })) << 1)
        let tmp = if delta > cyclic_pos { cyclic_size } else { 0 };
        let pair_idx = ((cyclic_pos.wrapping_sub(delta).wrapping_add(tmp)) as usize) * 2;
        if pos < delta {
            return match_count;
        }

        // SAFETY: read_pos >= delta (checked above). read_pos <= write_pos <= mf.size,
        // and buffer has LZMA_MEMCMPLEN_EXTRA = 8 extra bytes.
        let pb = unsafe { buf_ptr.add((mf.read_pos - delta) as usize) };
        let cur_ptr = unsafe { buf_ptr.add(mf.read_pos as usize) };

        let mut len = if len0 < len1 { len0 } else { len1 };

        // SAFETY: len < len_limit <= avail, and avail + LZMA_MEMCMPLEN_EXTRA <= buffer.len()
        if unsafe { *pb.add(len as usize) == *cur_ptr.add(len as usize) } {
            // SAFETY: buffer has LZMA_MEMCMPLEN_EXTRA extra bytes past avail, and
            // len_limit <= avail, so pb/cur_ptr + len_limit + 7 are in bounds.
            len = unsafe { lzma_memcmplen_unchecked(pb, cur_ptr, len + 1, len_limit) };
            if len_best < len {
                len_best = len;
                if let Some(m) = matches.get_mut(match_count) {
                    m.len = len;
                    m.dist = delta.wrapping_sub(1);
                }
                match_count += 1;
                if len == len_limit {
                    // SAFETY: ptr0, ptr1, pair_idx, pair_idx+1 < cyclic_size * 2 = sons_count
                    unsafe {
                        *son_ptr.add(ptr1) = *son_ptr.add(pair_idx);
                        *son_ptr.add(ptr0) = *son_ptr.add(pair_idx + 1);
                    }
                    return match_count;
                }
            }
        }
        // SAFETY: same bounds guarantee as above — len < len_limit <= avail
        if unsafe { *pb.add(len as usize) < *cur_ptr.add(len as usize) } {
            // SAFETY: ptr1, pair_idx+1 < cyclic_size * 2 = sons_count
            unsafe {
                *son_ptr.add(ptr1) = cur_match;
            }
            ptr1 = pair_idx + 1;
            // SAFETY: ptr1 (now = pair_idx+1) < cyclic_size * 2 = sons_count
            cur_match = unsafe { *son_ptr.add(ptr1) };
            len1 = len;
        } else {
            // SAFETY: ptr0, pair_idx < cyclic_size * 2 = sons_count
            unsafe {
                *son_ptr.add(ptr0) = cur_match;
            }
            ptr0 = pair_idx;
            // SAFETY: ptr0 (now = pair_idx) < cyclic_size * 2 = sons_count
            cur_match = unsafe { *son_ptr.add(ptr0) };
            len0 = len;
        }
    }
}

/// 根据 BT 算法跳过匹配查找过程中的指定数量
///
/// 参数说明：
/// - `len_limit`：匹配长度上限
/// - `pos`：当前数据在整个缓冲区中的位置（绝对位置）
/// - `cur`：整个数据缓冲区（包含当前数据段以及历史数据）
/// - `cur_match`：当前匹配候选位置（绝对位置）
/// - `depth`：查找深度
/// - `son`：辅助数组，用于存储匹配候选信息，长度应足够
/// - `cyclic_pos`：当前循环缓冲区位置
/// - `cyclic_size`：循环缓冲区总大小
pub fn bt_skip_func(len_limit: u32, pos: u32, mf: &mut LzmaMf, mut cur_match: u32) {
    let mut depth = mf.depth;
    let cyclic_pos = mf.cyclic_pos;
    let cyclic_size = mf.cyclic_size;

    let mut ptr0 = (cyclic_pos as usize) * 2 + 1;
    let mut ptr1 = (cyclic_pos as usize) * 2;
    let mut len0: u32 = 0;
    let mut len1: u32 = 0;

    // SAFETY: Same invariants as bt_find_func — buffer has LZMA_MEMCMPLEN_EXTRA extra bytes,
    // son has sons_count = cyclic_size * 2 entries, all accesses are within bounds.
    let buf_ptr = mf.buffer.as_ptr();
    let son_ptr = mf.son.as_mut_ptr();

    loop {
        let delta = pos - cur_match;
        if depth == 0 || delta >= cyclic_size {
            // SAFETY: ptr0, ptr1 < cyclic_size * 2 = sons_count
            unsafe {
                *son_ptr.add(ptr0) = 0;
                *son_ptr.add(ptr1) = 0;
            }
            return;
        }
        depth -= 1;

        let tmp = if delta > cyclic_pos { cyclic_size } else { 0 };
        let pair_idx = ((cyclic_pos.wrapping_sub(delta).wrapping_add(tmp)) as usize) * 2;
        if pos < delta {
            return;
        }

        // SAFETY: read_pos >= delta, buffer has LZMA_MEMCMPLEN_EXTRA extra bytes
        let pb = unsafe { buf_ptr.add((mf.read_pos - delta) as usize) };
        let cur_ptr = unsafe { buf_ptr.add(mf.read_pos as usize) };

        let mut len = if len0 < len1 { len0 } else { len1 };

        // SAFETY: len < len_limit <= avail, avail + LZMA_MEMCMPLEN_EXTRA is allocated
        if unsafe { *pb.add(len as usize) == *cur_ptr.add(len as usize) } {
            len = unsafe { lzma_memcmplen_unchecked(pb, cur_ptr, len + 1, len_limit) };
            if len == len_limit {
                // SAFETY: ptr0, ptr1, pair_idx, pair_idx+1 < cyclic_size * 2 = sons_count
                unsafe {
                    *son_ptr.add(ptr1) = *son_ptr.add(pair_idx);
                    *son_ptr.add(ptr0) = *son_ptr.add(pair_idx + 1);
                }
                return;
            }
        }
        // SAFETY: same bounds guarantee as above
        if unsafe { *pb.add(len as usize) < *cur_ptr.add(len as usize) } {
            // SAFETY: ptr1, pair_idx+1 < cyclic_size * 2 = sons_count
            unsafe {
                *son_ptr.add(ptr1) = cur_match;
            }
            ptr1 = pair_idx + 1;
            cur_match = unsafe { *son_ptr.add(ptr1) };
            len1 = len;
        } else {
            // SAFETY: ptr0, pair_idx < cyclic_size * 2 = sons_count
            unsafe {
                *son_ptr.add(ptr0) = cur_match;
            }
            ptr0 = pair_idx;
            cur_match = unsafe { *son_ptr.add(ptr0) };
            len0 = len;
        }
    }
}
/// 计算 16 位小端整数（对应 C 代码中的 read16ne）
fn read16ne(cur: &[u8]) -> u32 {
    // 若 buf 长度不足2，则返回 0；否则按小端顺序解析
    if cur.len() < 2 {
        0
    } else {
        u16::from_le_bytes([cur[0], cur[1]]) as u32
    }
}
/// 根据 BT 算法查找匹配项，并返回匹配项数目
pub fn lzma_mf_bt2_find(mf: &mut LzmaMf, matches: &mut [LzmaMatch]) -> u32 {
    // 获取可用字节数作为匹配长度的上限
    let mut len_limit = mf_avail(mf);
    if mf.nice_len <= len_limit {
        len_limit = mf.nice_len;
    } else if len_limit < 2 || (mf.action == LzmaAction::SyncFlush) {
        // 当可用字节不足2或者处于同步刷新状态时，更新状态后返回 0
        move_pending(mf);
        return 0;
    }
    let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
    let pos: u32 = mf.read_pos + mf.offset;
    let mut matches_count: u32 = 0;

    // 计算哈希值：temp = CRC32_TABLE[cur[0]] XOR cur[1]
    let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
    let hash_value = read16ne(cur);
    let cur_match = mf.hash[hash_value as usize];
    mf.hash[hash_value as usize] = pos;

    // 调用 bt_find_func 查找匹配项
    let found: usize = bt_find_func(
        len_limit,
        pos,
        mf,
        cur_match,
        &mut matches[matches_count as usize..],
        1, // 初始最佳匹配长度为 1
    );
    matches_count = found as u32;
    move_pos(mf);
    matches_count
}

/// 根据 BT 算法跳过匹配查找过程中的指定数量
pub fn lzma_mf_bt2_skip(mf: &mut LzmaMf, mut amount: u32) {
    while amount != 0 {
        let mut len_limit = mf_avail(mf);
        if mf.nice_len <= len_limit {
            len_limit = mf.nice_len;
        } else if len_limit < 2 || (mf.action == LzmaAction::SyncFlush) {
            move_pending(mf);
            amount -= 1;
            continue;
        }
        let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
        let pos: u32 = mf.read_pos + mf.offset;
        let hash_value = read16ne(cur);
        let cur_match = mf.hash[hash_value as usize];
        mf.hash[hash_value as usize] = pos;

        bt_skip_func(len_limit, pos, mf, cur_match);
        move_pos(mf);
        amount -= 1;
    }
}

/// 根据 BT 算法查找匹配项，并返回匹配项数目
///
/// 参数说明：
/// - `len_limit`: 匹配长度上限
/// - `pos`: 当前数据在整个缓冲区中的绝对位置
/// - `cur`: 整个数据缓冲区切片（包含当前段和历史数据）
/// - `cur_match`: 当前匹配候选位置（绝对位置）
/// - `depth`: 查找深度
/// - `son`: 辅助数组，用于存储匹配候选信息，长度应足够
/// - `cyclic_pos`: 当前循环缓冲区位置
/// - `cyclic_size`: 循环缓冲区总大小
/// - `matches`: 用于存储匹配结果的数组切片，调用者保证容量足够
/// - `len_best`: 当前已找到的最佳匹配长度
///
/// 返回值：匹配结果数组中已写入的匹配项数量（usize）
pub fn lzma_mf_bt3_find(mf: &mut LzmaMf, matches: &mut [LzmaMatch]) -> u32 {
    // 取可用字节数作为匹配长度上限
    let mut len_limit = mf_avail(mf);
    if mf.nice_len <= len_limit {
        len_limit = mf.nice_len;
    } else if len_limit < 3 || (mf.action == LzmaAction::SyncFlush) {
        // 当可用字节不足 3 或处于同步刷新状态时，更新状态后返回 0
        move_pending(mf);
        return 0;
    }
    // 获取当前数据缓冲区切片，保证安全，不使用 unsafe
    let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
    let pos: u32 = mf.read_pos + mf.offset;
    let mut matches_count: u32 = 0;

    // 计算哈希值：temp = CRC32_TABLE[cur[0]] XOR cur[1]
    let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
    // 计算哈希相关值
    let hash_2_value: u32 = temp & (((1u32) << 10) - 1);
    // 此处采用 read16ne 计算hash_value，等同于： (temp ^ (cur[2]<<8)) & mf.hash_mask
    let hash_value: u32 = (temp ^ ((cur[2] as u32) << 8)) & mf.hash_mask;
    // 计算 delta2：当前 pos 与 hash 中保存的值差值
    let delta2: u32 = pos - mf.hash[hash_2_value as usize];
    // 取当前候选匹配：存于 hash 中偏移 (1<<10) + hash_value
    let cur_match: u32 = mf.hash[((1u32 << 10) as usize) + hash_value as usize];
    // 更新全局 hash 数组
    mf.hash[hash_2_value as usize] = pos;
    mf.hash[((1u32 << 10) as usize) + hash_value as usize] = pos;
    let mut len_best: u32 = 2;
    // 取当前数据在整体缓冲区中的起始索引（必须保证存在，否则取 0）
    let cur_index: usize = mf.cur_offset() as usize;

    // 判断：如果 delta2 小于循环缓冲区大小，且前一个字节与当前数据第一个字节相同
    // 对应 C 代码：if (delta2 < mf->cyclic_size && *(cur - delta2) == *cur)
    if delta2 < mf.cyclic_size
        && cur_index >= (delta2 as usize)
        && cur[cur_index - delta2 as usize] == cur[cur_index]
    {
        // 调用 lzma_memcmplen 计算匹配长度（传入切片：当前数据切片和前移 delta2 后的数据切片）
        len_best = lzma_memcmplen(
            &cur[cur_index..],
            &cur[cur_index - delta2 as usize..],
            len_best,
            len_limit,
        );
        if let Some(first_match) = matches.get_mut(0) {
            first_match.len = len_best;
            first_match.dist = delta2.wrapping_sub(1);
        }
        matches_count = 1;
        if len_best == len_limit {
            // 如果匹配长度达到上限，则调用 bt_skip_func 更新匹配器状态后返回
            bt_skip_func(len_limit, pos, mf, cur_match);
            move_pos(mf);
            return 1;
        }
    }
    // 调用 bt_find_func 查找更多匹配项，返回写入 matches 切片的匹配项数
    let found = bt_find_func(
        len_limit,
        pos,
        mf,
        cur_match,
        &mut matches[matches_count as usize..],
        len_best,
    );
    // 计算总匹配个数（found 为 usize，与 matches_count 相加转换为 u32）
    matches_count += found as u32;
    move_pos(mf);
    matches_count
}

/// 根据 BT 算法跳过匹配查找过程中的指定数量
///
/// 参数说明：
/// - `mf`: 匹配查找器对象
/// - `amount`: 需要跳过的匹配数量
pub fn lzma_mf_bt3_skip(mf: &mut LzmaMf, mut amount: u32) {
    // 循环跳过指定数量
    while amount != 0 {
        let mut len_limit = mf_avail(mf);
        if mf.nice_len <= len_limit {
            len_limit = mf.nice_len;
        } else if len_limit < 3 || (mf.action == LzmaAction::SyncFlush) {
            move_pending(mf);
            // 跳过当前数据后减少 amount，继续下一次循环
            amount -= 1;
            continue;
        }
        let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
        let pos: u32 = mf.read_pos + mf.offset;
        let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
        let hash_2_value: u32 = temp & (((1u32) << 10) - 1);
        let hash_value: u32 = (temp ^ ((cur[2] as u32) << 8)) & mf.hash_mask;
        let cur_match: u32 = mf.hash[((1u32 << 10) as usize) + hash_value as usize];
        // 更新全局 hash 数组
        mf.hash[hash_2_value as usize] = pos;
        mf.hash[((1u32 << 10) as usize) + hash_value as usize] = pos;

        // 调用 bt_skip_func 更新匹配查找状态
        bt_skip_func(len_limit, pos, mf, cur_match);
        move_pos(mf);
        amount -= 1;
    }
}

/// 根据 BT 算法查找匹配项，并返回匹配项数目
///
/// 参数说明：
/// - `len_limit`: 匹配长度上限
/// - `pos`: 当前数据在整个缓冲区中的绝对位置
/// - `cur`: 整个数据缓冲区切片（包含当前段和历史数据）
/// - `cur_match`: 当前匹配候选位置（绝对位置）
/// - `depth`: 查找深度
/// - `son`: 辅助数组，用于存储匹配候选信息，长度应足够
/// - `cyclic_pos`: 当前循环缓冲区位置
/// - `cyclic_size`: 循环缓冲区总大小
/// - `matches`: 用于存储匹配结果的数组切片，调用者保证容量足够
/// - `len_best`: 当前已找到的最佳匹配长度
///
/// 返回值：匹配结果数组中写入的匹配项数量（u32）
pub fn lzma_mf_bt4_find(mf: &mut LzmaMf, matches: &mut [LzmaMatch]) -> u32 {
    // 取可用字节数作为匹配长度上限
    let mut len_limit = mf_avail(mf);
    if mf.nice_len <= len_limit {
        len_limit = mf.nice_len;
    } else if len_limit < 4 || (mf.action == LzmaAction::SyncFlush) {
        // 当可用字节不足4或处于同步刷新状态时，更新状态后返回0
        move_pending(mf);
        return 0;
    }
    // 获取当前数据缓冲区切片，安全返回，无需 unsafe
    let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
    let pos: u32 = mf.read_pos + mf.offset;
    let mut matches_count: u32 = 0;

    // 计算哈希值：temp = CRC32_TABLE[cur[0]] XOR cur[1]
    let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
    // 计算哈希相关值
    let hash_2_value: u32 = temp & (((1u32) << 10) - 1);
    let hash_3_value: u32 = (temp ^ ((cur[2] as u32) << 8)) & (((1u32) << 16) - 1);
    // 计算 hash_value: 等同于 (temp ^ (cur[2]<<8) ^ (CRC32_TABLE[cur[3]] << 5)) & mf.hash_mask
    let hash_value: u32 =
        (temp ^ ((cur[2] as u32) << 8) ^ (CRC32_TABLE[cur[3] as usize] << 5)) & mf.hash_mask;

    // 计算 delta2：当前 pos 与 hash 表中对应值之差
    let mut delta2: u32 = pos - mf.hash[hash_2_value as usize];
    // 计算 delta3：当前 pos 与 hash[(1<<10) + hash_3_value] 之间的差值
    let delta3: u32 = pos - mf.hash[(((1u32) << 10) as usize + hash_3_value as usize)];
    // 取当前候选匹配值：存放在 hash[(1<<10) + (1<<16) + hash_value]
    let cur_match: u32 =
        mf.hash[(((1u32) << 10) as usize + ((1u32) << 16) as usize + hash_value as usize)];
    // 更新 hash 表
    mf.hash[hash_2_value as usize] = pos;
    mf.hash[(((1u32) << 10) as usize) + hash_3_value as usize] = pos;
    mf.hash[(((1u32) << 10) as usize + ((1u32) << 16) as usize + hash_value as usize)] = pos;

    let mut len_best: u32 = 1;
    // 获取当前数据在整体缓冲区中的起始索引（若不存在则取0）
    let cur_index: usize = mf.cur_offset() as usize;

    // 判断：如果 delta2 小于循环大小，且前移 delta2 后的字节与当前字节相同

    if delta2 < mf.cyclic_size
        && cur_index >= (delta2 as usize)
        && cur[0] == mf.buffer[(mf.cur_offset() - delta2) as usize]
    {
        let a = cur[0];
        let b = mf.buffer[(mf.cur_offset() - delta2) as usize];
        len_best = 2;
        if let Some(first_match) = matches.get_mut(0) {
            first_match.len = 2;
            first_match.dist = delta2.wrapping_sub(1);
        }
        matches_count = 1;
    }
    // 判断：如果 delta2 与 delta3 不相等，且 delta3 小于循环大小，且前移 delta3 后的字节与当前字节相同，
    // 则更新最佳匹配长度为 3，并记录匹配结果，同时更新 delta2 为 delta3
    if delta2 != delta3
        && delta3 < mf.cyclic_size
        && cur_index >= (delta3 as usize)
        && mf.buffer[(mf.cur_offset() - delta3) as usize] == cur[0]
    {
        len_best = 3;
        if let Some(m) = matches.get_mut(matches_count as usize) {
            m.dist = delta3.wrapping_sub(1);
        }
        matches_count += 1;
        delta2 = delta3;
    }
    // 如果已有匹配项，则计算匹配长度更新最佳匹配长度
    if matches_count != 0 {
        // 使用 lzma_memcmplen 计算匹配长度：传入两个切片分别为当前数据段和前移 delta2 后的数据段
        len_best = lzma_memcmplen(
            &cur,
            &mf.buffer[(mf.read_pos - delta2) as usize..],
            len_best,
            len_limit,
        );
        if let Some(last_match) = matches.get_mut((matches_count - 1) as usize) {
            last_match.len = len_best;
        }
        if len_best == len_limit {
            // 如果匹配长度达到上限，则调用 bt_skip_func 更新匹配器状态后直接返回匹配数
            bt_skip_func(len_limit, pos, mf, cur_match);
            move_pos(mf);
            return matches_count;
        }
    }
    if len_best < 3 {
        len_best = 3;
    }
    // 调用 bt_find_func 查找更多匹配项，返回写入匹配结果数组的起始匹配项数量
    let found = bt_find_func(
        len_limit,
        pos,
        mf,
        cur_match,
        &mut matches[matches_count as usize..],
        len_best,
    );
    matches_count += found as u32; // 匹配的位置
    move_pos(mf);
    matches_count
}

/// 根据 BT 算法跳过匹配查找过程中的指定数量
///
/// 参数说明：
/// - `mf`: 匹配查找器对象
/// - `amount`: 需要跳过的匹配项数量
pub fn lzma_mf_bt4_skip(mf: &mut LzmaMf, mut amount: u32) {
    while amount != 0 {
        let mut len_limit = mf_avail(mf);
        if mf.nice_len <= len_limit {
            len_limit = mf.nice_len;
        } else if len_limit < 4 || (mf.action == LzmaAction::SyncFlush) {
            move_pending(mf);
            amount -= 1;
            continue;
        }
        let cur: &[u8] = &mf.buffer[mf.read_pos as usize..];
        let pos: u32 = mf.read_pos + mf.offset;
        let temp: u32 = CRC32_TABLE[cur[0] as usize] ^ (cur[1] as u32);
        let hash_2_value: u32 = temp & (((1u32) << 10) - 1);
        let hash_3_value: u32 = (temp ^ ((cur[2] as u32) << 8)) & (((1u32) << 16) - 1);
        let hash_value: u32 =
            (temp ^ ((cur[2] as u32) << 8) ^ (CRC32_TABLE[cur[3] as usize] << 5)) & mf.hash_mask;
        let cur_match: u32 =
            mf.hash[((1u32 << 10) as usize) + ((1u32 << 16) as usize) + hash_value as usize];
        mf.hash[hash_2_value as usize] = pos;
        mf.hash[((1u32 << 10) as usize) + hash_3_value as usize] = pos;
        mf.hash[((1u32 << 10) as usize) + ((1u32 << 16) as usize) + hash_value as usize] = pos;
        // 调用 bt_skip_func 更新匹配查找状态
        bt_skip_func(len_limit, pos, mf, cur_match);
        move_pos(mf);
        amount -= 1;
    }
}

// macro_rules! header {
//     ($is_bt:expr, $len_min:expr, $ret_op:expr) => {
//         let mut len_limit = mf_avail(mf);
//         if mf.nice_len <= len_limit {
//             len_limit = mf.nice_len;
//         } else if len_limit < $len_min || ($is_bt && mf.action == LzmaAction::SyncFlush) {
//             assert!(mf.action != LzmaAction::Run);
//             move_pending(mf);
//             $ret_op;
//         }
//         let cur = mf_ptr(mf);
//         let pos = mf.read_pos + mf.offset;
//     };
// }

// macro_rules! header_find {
//     ($is_bt:expr, $len_min:expr) => {
//         header!($is_bt, $len_min, return 0);
//         let mut matches_count = 0;
//     };
// }

// macro_rules! header_skip {
//     ($is_bt:expr, $len_min:expr) => {
//         header!($is_bt, $len_min, continue);
//     };
// }

// macro_rules! call_find {
//     ($func:expr, $len_best:expr) => {
//         matches_count = ($func(
//             len_limit,
//             pos,
//             cur,
//             cur_match,
//             mf.depth,
//             mf.son,
//             mf.cyclic_pos,
//             mf.cyclic_size,
//             &mut matches[matches_count as usize..],
//             $len_best,
//         ) - matches) as u32;
//         move_pos(mf);
//         return matches_count;
//     };
// }
