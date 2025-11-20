/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

 use crate::{api::LzmaAction, check::LZMA_CRC32_TABLE, common::lzma_memcmplen, lz::mf_ptr};

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
     let temp = LZMA_CRC32_TABLE.lock().unwrap()[0][cur[0] as usize] ^ cur[1] as u32;
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
         let temp = LZMA_CRC32_TABLE.lock().unwrap()[0][cur[0] as usize] ^ (cur[1] as u32);
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
 