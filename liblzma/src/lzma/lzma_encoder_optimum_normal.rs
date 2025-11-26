/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

 use common::{my_max, my_min};

 use crate::{
     common::lzma_memcmplen,
     get_dist_state,
     lz::{lzma_mf_find, mf_avail, mf_skip, LzmaMf},
     lzma::OPTS,
     not_equal_16,
     rangecoder::{
         rc_bit_0_price, rc_bit_1_price, rc_bit_price, rc_bittree_price, rc_bittree_reverse_price,
         rc_direct_price, RC_INFINITY_PRICE,
     },
 };
 
 use super::{
     get_dist_slot, get_dist_slot_2, is_literal_state, update_literal, update_long_rep,
     update_match, update_short_rep, LzmaLengthEncoder, LzmaLzma1Encoder, LzmaOptimal, ALIGN_BITS,
     ALIGN_MASK, ALIGN_SIZE, DIST_MODEL_END, DIST_MODEL_START, DIST_SLOT_BITS, DIST_STATES,
     FULL_DISTANCES, MATCH_LEN_MAX, MATCH_LEN_MIN, REPS,
 };
 use crate::lzma::LIT_STATES;
 fn get_literal_price(
     coder: &LzmaLzma1Encoder,
     pos: u32,
     prev_byte: u32,
     match_mode: bool,
     mut match_byte: u32,
     mut symbol: u32,
 ) -> u32 {
     let index = (((pos) & (coder.literal_pos_mask)) << (coder.literal_context_bits))
         + ((prev_byte) >> (8 - (coder.literal_context_bits)));
     let subcoder = &(coder.literal)[index as usize];
 
     let mut price = 0;
 
     if !match_mode {
         price = rc_bittree_price(subcoder, 8, symbol);
     } else {
         let mut offset = 0x100;
         symbol += 1 << 8;
 
         while symbol < (1 << 16) {
             match_byte <<= 1;
 
             let match_bit = match_byte & offset;
             let subcoder_index = offset + match_bit + (symbol >> 8);
             let bit = (symbol >> 7) & 1;
             let prob = *subcoder[subcoder_index as usize].lock().unwrap();
             price += rc_bit_price(prob, bit);
 
             symbol <<= 1;
             offset &= !(match_byte ^ symbol);
         }
     }
 
     price
 }
 
 fn get_len_price(lencoder: &LzmaLengthEncoder, len: u32, pos_state: u32) -> u32 {
     // 注意：与其他价格表不同，长度价格在 lzma_encoder.c 中更新
     lencoder.prices[pos_state as usize][(len as usize - MATCH_LEN_MIN)]
 }
 
 /// 获取短期回退价格
 fn get_short_rep_price(coder: &LzmaLzma1Encoder, state: u32, pos_state: u32) -> u32 {
     rc_bit_0_price(*coder.is_rep0[state as usize].lock().unwrap())
         + rc_bit_0_price(
             *coder.is_rep0_long[state as usize][pos_state as usize]
                 .lock()
                 .unwrap(),
         )
 }
 
 /// 获取纯回退价格
 fn get_pure_rep_price(coder: &LzmaLzma1Encoder, rep_index: u32, state: u32, pos_state: u32) -> u32 {
     let mut price: u32;
 
     if rep_index == 0 {
         price = rc_bit_0_price(*coder.is_rep0[state as usize].lock().unwrap());
         price += rc_bit_1_price(
             *coder.is_rep0_long[state as usize][pos_state as usize]
                 .lock()
                 .unwrap(),
         );
     } else {
         price = rc_bit_1_price(*coder.is_rep0[state as usize].lock().unwrap());
 
         if rep_index == 1 {
             price += rc_bit_0_price(*coder.is_rep1[state as usize].lock().unwrap());
         } else {
             price += rc_bit_1_price(*coder.is_rep1[state as usize].lock().unwrap());
             price += rc_bit_price(
                 *coder.is_rep2[state as usize].lock().unwrap(),
                 rep_index - 2,
             );
         }
     }
 
     price
 }
 
 /// 获取回退价格
 fn get_rep_price(
     coder: &LzmaLzma1Encoder,
     rep_index: u32,
     len: u32,
     state: u32,
     pos_state: u32,
 ) -> u32 {
     get_len_price(&coder.rep_len_encoder, len, pos_state)
         + get_pure_rep_price(coder, rep_index, state, pos_state)
 }
 
 /// 获取距离长度的价格
 fn get_dist_len_price(coder: &mut LzmaLzma1Encoder, dist: u32, len: u32, pos_state: u32) -> u32 {
     let dist_state = get_dist_state!(len);
     let mut price: u32;
 
     if dist < FULL_DISTANCES as u32 {
         price = coder.dist_prices[dist_state as usize][dist as usize];
     } else {
         let dist_slot = get_dist_slot_2(dist);
         price = coder.dist_slot_prices[dist_state as usize][dist_slot as usize]
             + coder.align_prices[(dist & ALIGN_MASK as u32) as usize];
     }
 
     price += get_len_price(&coder.match_len_encoder, len, pos_state);
 
     price
 }
 
 /// 填充距离价格
 fn fill_dist_prices(coder: &mut LzmaLzma1Encoder) {
     for dist_state in 0..DIST_STATES {
         let dist_slot_prices: &mut [u32] = &mut coder.dist_slot_prices[dist_state as usize];
 
         // 编码 dist_slot 的价格
         for dist_slot in 0..coder.dist_table_size {
             dist_slot_prices[dist_slot as usize] = rc_bittree_price(
                 &coder.dist_slot[dist_state as usize],
                 DIST_SLOT_BITS as u32,
                 dist_slot,
             );
         }
 
         // 对于距离大于等于 FULL_DISTANCES 的匹配，添加直接位部分的价格
         // (对齐位由 fill_align_prices 处理)
         for dist_slot in DIST_MODEL_END..coder.dist_table_size as usize {
             dist_slot_prices[dist_slot as usize] +=
                 rc_direct_price((((dist_slot >> 1) - 1) - ALIGN_BITS) as u32);
         }
 
         // 对于距离在 [0, 3] 范围内的匹配，直接使用 dist_slot
         // 将它们用于 coder.dist_prices
         for i in 0..DIST_MODEL_START {
             coder.dist_prices[dist_state as usize][i as usize] = dist_slot_prices[i as usize];
         }
     }
 
     // 对于距离在 [4, 127] 范围内的匹配，依赖于 dist_slot 和 dist_special
     // 使用单独的循环，避免重复调用 get_dist_slot()
     for i in DIST_MODEL_START..FULL_DISTANCES {
         let dist_slot = get_dist_slot(i as u32);
         let footer_bits = ((dist_slot >> 1) - 1);
         let base = (2 | (dist_slot & 1)) << footer_bits;
         let dist_reduced = (i as u32 - base);
 
         let index_i32 = base as i32 - dist_slot as i32 - 1;
         let (slice_start, flags) = if index_i32 < 0 {
             (0, 1) // 负索引情况，使用flags=1
         } else {
             (index_i32 as usize, 0)
         };
 
         let price = rc_bittree_reverse_price(
             &coder.dist_special[slice_start..],
             footer_bits,
             dist_reduced,
             flags as u8,
         );
 
         for dist_state in 0..DIST_STATES {
             coder.dist_prices[dist_state as usize][i as usize] =
                 price + coder.dist_slot_prices[dist_state as usize][dist_slot as usize];
         }
     }
 
     coder.match_price_count = 0;
 }
 
 /// 填充对齐价格
 fn fill_align_prices(coder: &mut LzmaLzma1Encoder) {
     for i in 0..ALIGN_SIZE {
         coder.align_prices[i as usize] =
             rc_bittree_reverse_price(&coder.dist_align, ALIGN_BITS as u32, i as u32, 0);
     }
 
     coder.align_price_count = 0;
 }
 /////////////
 // Optimal //
 /////////////
 
 /// 设置为字面值
 fn make_literal(optimal: &mut LzmaOptimal) {
     optimal.back_prev = u32::MAX;
     optimal.prev_1_is_literal = false;
 }
 
 /// 设置为短期重复
 fn make_short_rep(optimal: &mut LzmaOptimal) {
     optimal.back_prev = 0;
     optimal.prev_1_is_literal = false;
 }
 
 /// 检查是否是短期重复
 fn is_short_rep(optimal: &LzmaOptimal) -> bool {
     optimal.back_prev == 0
 }
 
 /// 后退操作
 fn backward(coder: &mut LzmaLzma1Encoder, len_res: &mut u32, back_res: &mut u32, mut cur: u32) {
     coder.opts_end_index = cur;
 
     let mut pos_mem = coder.opts[cur as usize].pos_prev;
     let mut back_mem = coder.opts[cur as usize].back_prev;
 
     loop {
         if coder.opts[cur as usize].prev_1_is_literal {
             make_literal(&mut coder.opts[pos_mem as usize]);
             coder.opts[pos_mem as usize].pos_prev = pos_mem - 1;
 
             if coder.opts[cur as usize].prev_2 {
                 coder.opts[pos_mem as usize - 1].prev_1_is_literal = false;
                 coder.opts[pos_mem as usize - 1].pos_prev = coder.opts[cur as usize].pos_prev_2;
                 coder.opts[pos_mem as usize - 1].back_prev = coder.opts[cur as usize].back_prev_2;
             }
         }
 
         let pos_prev = pos_mem;
         let back_cur = back_mem;
 
         back_mem = coder.opts[pos_prev as usize].back_prev;
         pos_mem = coder.opts[pos_prev as usize].pos_prev;
 
         coder.opts[pos_prev as usize].back_prev = back_cur;
         coder.opts[pos_prev as usize].pos_prev = cur;
         cur = pos_prev;
 
         if cur == 0 {
             break;
         }
     }
 
     coder.opts_current_index = coder.opts[0].pos_prev;
     *len_res = coder.opts[0].pos_prev;
     *back_res = coder.opts[0].back_prev;
 }
 
 //////////
 // Main //
 //////////
 
 /// 辅助函数 1
 fn helper1(
     coder: &mut LzmaLzma1Encoder,
     mf: &mut LzmaMf,
     back_res: &mut u32,
     len_res: &mut u32,
     position: u32,
 ) -> u32 {
     // 获取 nice_len
     let nice_len = mf.nice_len;
 
     let mut len_main: u32 = 0;
     let mut matches_count: u32 = 0;
 
     // 根据 read_ahead 的值决定如何获取主匹配长度
     if mf.read_ahead == 0 {
         len_main = lzma_mf_find(mf, &mut matches_count, &mut coder.matches);
     } else {
         assert!(mf.read_ahead == 1);
         len_main = coder.longest_match_length;
         matches_count = coder.matches_count;
     }
 
     // 计算可用缓冲区大小
     let buf_avail = my_min(mf_avail(mf) + 1, MATCH_LEN_MAX as u32);
     if buf_avail < 2 {
         *back_res = u32::MAX;
         *len_res = 1;
         return u32::MAX;
     }
 
     // 获取缓冲区指针
     let buf = &mf.buffer[mf.mf_ptr(1)..];
 
     // 初始化重复长度数组和最大索引
     let mut rep_lens = [0u32; REPS];
     let mut rep_max_index = 0;
 
     // 计算每个重复的长度
     for i in 0..REPS {
         // let buf_back = buf.wrapping_sub(coder.reps[i] as usize + 1);
         let buf_back = &mf.buffer[mf.mf_ptr(1) - coder.reps[i] as usize - 1..];
         if not_equal_16!(buf, buf_back) {
             rep_lens[i] = 0;
             continue;
         }
 
         rep_lens[i] = lzma_memcmplen(buf, buf_back, 2, buf_avail);
 
         if rep_lens[i] > rep_lens[rep_max_index] {
             rep_max_index = i;
         }
     }
 
     // 如果最大重复长度大于等于 nice_len，更新结果并返回
     if rep_lens[rep_max_index] >= nice_len {
         *back_res = rep_max_index as u32;
         *len_res = rep_lens[rep_max_index];
         mf_skip(mf, *len_res - 1);
         return u32::MAX;
     }
 
     // 如果主匹配长度大于等于 nice_len，更新结果并返回
     if len_main >= nice_len {
         *back_res = coder.matches[matches_count as usize - 1].dist + REPS as u32;
         *len_res = len_main;
         mf_skip(mf, len_main - 1);
         return u32::MAX;
     }
 
     // 获取当前字节和匹配字节
     let current_byte: u8 = buf[0];
     // let match_byte = *(buf.wrapping_sub(coder.reps[0] as usize + 1));
     let match_byte = mf.buffer[mf.mf_ptr(1) as usize - coder.reps[0] as usize - 1];
 
     // 如果主匹配长度小于 2 且当前字节不等于匹配字节且最大重复长度小于 2，更新结果并返回
     if len_main < 2 && current_byte != match_byte && rep_lens[rep_max_index] < 2 {
         *back_res = u32::MAX;
         *len_res = 1;
         return u32::MAX;
     }
 
     // 初始化编码器状态
     coder.opts[0].state = coder.state;
 
     // 计算位置状态
     let pos_state = position & coder.pos_mask;
 
     // 计算字面值价格
     coder.opts[1].price = rc_bit_0_price(
         *coder.is_match[coder.state as usize][pos_state as usize]
             .lock()
             .unwrap(),
     ) + get_literal_price(
         coder,
         position,
         mf.buffer[mf.mf_ptr(1) - 1] as u32,
         !is_literal_state(coder.state),
         match_byte as u32,
         current_byte as u32,
     );
 
     make_literal(&mut coder.opts[1]);
 
     // 计算匹配价格和重复匹配价格
     let match_price = rc_bit_1_price(
         *coder.is_match[coder.state as usize][pos_state as usize]
             .lock()
             .unwrap(),
     );
     let rep_match_price =
         match_price + rc_bit_1_price(*coder.is_rep[coder.state as usize].lock().unwrap());
 
     // 如果匹配字节等于当前字节，计算短重复价格并更新
     if match_byte == current_byte {
         let short_rep_price = rep_match_price + get_short_rep_price(coder, coder.state, pos_state);
 
         if short_rep_price < coder.opts[1].price {
             coder.opts[1].price = short_rep_price;
             make_short_rep(&mut coder.opts[1]);
         }
     }
 
     // 计算结束长度
     let len_end = my_max(len_main, rep_lens[rep_max_index]);
 
     // 如果结束长度小于 2，更新结果并返回
     if len_end < 2 {
         *back_res = coder.opts[1].back_prev;
         *len_res = 1;
         return u32::MAX;
     }
 
     coder.opts[1].pos_prev = 0;
 
     // 更新编码器的 backs 数组
     for i in 0..REPS {
         coder.opts[0].backs[i] = coder.reps[i];
     }
 
     // 初始化每个长度的价格为无穷大
     let mut len = len_end;
     while len >= 2 {
         coder.opts[len as usize].price = RC_INFINITY_PRICE;
         len -= 1;
     }
 
     // 计算每个重复的价格
     for i in 0..REPS {
         let mut rep_len = rep_lens[i];
         if rep_len < 2 {
             continue;
         }
 
         let price = rep_match_price + get_pure_rep_price(coder, i as u32, coder.state, pos_state);
 
         while rep_len >= 2 {
             let cur_and_len_price =
                 price + get_len_price(&coder.rep_len_encoder, rep_len, pos_state);
 
             if cur_and_len_price < coder.opts[rep_len as usize].price {
                 coder.opts[rep_len as usize].price = cur_and_len_price;
                 coder.opts[rep_len as usize].pos_prev = 0;
                 coder.opts[rep_len as usize].back_prev = i as u32;
                 coder.opts[rep_len as usize].prev_1_is_literal = false;
             }
             rep_len -= 1;
         }
     }
 
     // 计算正常匹配价格
     let normal_match_price =
         match_price + rc_bit_0_price(*coder.is_rep[coder.state as usize].lock().unwrap());
 
     len = if rep_lens[0] >= 2 { rep_lens[0] + 1 } else { 2 };
     if len <= len_main {
         let mut i = 0;
         while len > coder.matches[i].len {
             i += 1;
         }
 
         loop {
             let dist = coder.matches[i].dist;
             let cur_and_len_price =
                 normal_match_price + get_dist_len_price(coder, dist, len, pos_state);
 
             if cur_and_len_price < coder.opts[len as usize].price {
                 coder.opts[len as usize].price = cur_and_len_price;
                 coder.opts[len as usize].pos_prev = 0;
                 coder.opts[len as usize].back_prev = dist + REPS as u32;
                 coder.opts[len as usize].prev_1_is_literal = false;
             }
 
             if len == coder.matches[i].len {
                 if i + 1 == matches_count as usize {
                     break;
                 }
                 i += 1;
             }
             len += 1;
         }
     }
 
     len_end
 }
 
 /// 辅助函数 2
 fn helper2(
     coder: &mut LzmaLzma1Encoder,
     reps: &mut [u32; REPS],
     mf: &mut LzmaMf,
     mut len_end: u32,
     position: u32,
     cur: u32,
     buf_avail_full: u32,
 ) -> u32 {
     let nice_len = mf.nice_len;
     let mut matches_count = coder.matches_count;
     let mut new_len = coder.longest_match_length;
     let mut pos_prev = coder.opts[cur as usize].pos_prev;
     let mut state: u32;
 
     if coder.opts[cur as usize].prev_1_is_literal {
         pos_prev -= 1;
 
         if coder.opts[cur as usize].prev_2 {
             state = coder.opts[coder.opts[cur as usize].pos_prev_2 as usize].state;
 
             if coder.opts[cur as usize].back_prev_2 < REPS as u32 {
                 update_long_rep(&mut state);
             } else {
                 update_match(&mut state);
             }
         } else {
             state = coder.opts[pos_prev as usize].state;
         }
 
         state = update_literal(state);
     } else {
         state = coder.opts[pos_prev as usize].state;
     }
 
     if pos_prev == cur - 1 {
         if is_short_rep(&coder.opts[cur as usize]) {
             update_short_rep(&mut state);
         } else {
             state = update_literal(state);
         }
     } else {
         let mut pos;
         if coder.opts[cur as usize].prev_1_is_literal && coder.opts[cur as usize].prev_2 {
             pos_prev = coder.opts[cur as usize].pos_prev_2;
             pos = coder.opts[cur as usize].back_prev_2;
             update_long_rep(&mut state);
         } else {
             pos = coder.opts[cur as usize].back_prev;
             if pos < REPS as u32 {
                 update_long_rep(&mut state);
             } else {
                 update_match(&mut state);
             }
         }
 
         if pos < REPS as u32 {
             reps[0] = coder.opts[pos_prev as usize].backs[pos as usize];
 
             for i in 1..=pos as usize {
                 reps[i] = coder.opts[pos_prev as usize].backs[i - 1];
             }
 
             for i in (pos as usize + 1)..REPS {
                 reps[i] = coder.opts[pos_prev as usize].backs[i];
             }
         } else {
             reps[0] = pos - REPS as u32;
 
             for i in 1..REPS {
                 reps[i] = coder.opts[pos_prev as usize].backs[i - 1];
             }
         }
     }
 
     coder.opts[cur as usize].state = state;
 
     for i in 0..REPS {
         coder.opts[cur as usize].backs[i] = reps[i];
     }
 
     let cur_price = coder.opts[cur as usize].price;
     let buf = &mf.buffer[mf.mf_ptr(1)..];
     let current_byte = buf[0];
     let match_byte = mf.buffer[mf.mf_ptr(1).wrapping_sub(reps[0] as usize + 1)];
 
     let pos_state = position & coder.pos_mask;
 
     let cur_and_1_price = cur_price
         + rc_bit_0_price(
             *coder.is_match[state as usize][pos_state as usize]
                 .lock()
                 .unwrap(),
         )
         + get_literal_price(
             coder,
             position,
             mf.buffer[mf.mf_ptr(1).wrapping_sub(1)] as u32,
             !is_literal_state(state),
             match_byte as u32,
             current_byte as u32,
         );
 
     let mut next_is_literal = false;
 
     if cur_and_1_price < coder.opts[(cur + 1) as usize].price {
         coder.opts[(cur + 1) as usize].price = cur_and_1_price;
         coder.opts[(cur + 1) as usize].pos_prev = cur;
         make_literal(&mut coder.opts[(cur + 1) as usize]);
         next_is_literal = true;
     }
 
     let match_price = cur_price
         + rc_bit_1_price(
             *coder.is_match[state as usize][pos_state as usize]
                 .lock()
                 .unwrap(),
         );
     let rep_match_price =
         match_price + rc_bit_1_price(*coder.is_rep[state as usize].lock().unwrap());
 
     if match_byte == current_byte
         && !(coder.opts[(cur + 1) as usize].pos_prev < cur
             && coder.opts[(cur + 1) as usize].back_prev == 0)
     {
         let short_rep_price = rep_match_price + get_short_rep_price(coder, state, pos_state);
 
         if short_rep_price <= coder.opts[(cur + 1) as usize].price {
             coder.opts[(cur + 1) as usize].price = short_rep_price;
             coder.opts[(cur + 1) as usize].pos_prev = cur;
             make_short_rep(&mut coder.opts[(cur + 1) as usize]);
             next_is_literal = true;
         }
     }
 
     if buf_avail_full < 2 {
         return len_end;
     }
 
     let buf_avail = my_min(buf_avail_full, nice_len);
 
     if !next_is_literal && match_byte != current_byte {
         let buf_back = &mf.buffer[mf.mf_ptr(1).wrapping_sub(reps[0] as usize + 1)..];
         let limit = my_min(buf_avail_full, nice_len + 1);
 
         let len_test = lzma_memcmplen(buf, buf_back, 1, limit) - 1;
 
         if len_test >= 2 {
             let mut state_2 = state;
             state_2 = update_literal(state_2);
 
             let pos_state_next = (position + 1) & coder.pos_mask;
             let next_rep_match_price = cur_and_1_price
                 + rc_bit_1_price(
                     *coder.is_match[state_2 as usize][pos_state_next as usize]
                         .lock()
                         .unwrap(),
                 )
                 + rc_bit_1_price(*coder.is_rep[state_2 as usize].lock().unwrap());
 
             let offset = cur + 1 + len_test;
 
             while len_end < offset {
                 coder.opts[(len_end + 1) as usize].price = RC_INFINITY_PRICE;
                 len_end += 1;
             }
 
             let cur_and_len_price =
                 next_rep_match_price + get_rep_price(coder, 0, len_test, state_2, pos_state_next);
 
             if cur_and_len_price < coder.opts[offset as usize].price {
                 coder.opts[offset as usize].price = cur_and_len_price;
                 coder.opts[offset as usize].pos_prev = cur + 1;
                 coder.opts[offset as usize].back_prev = 0;
                 coder.opts[offset as usize].prev_1_is_literal = true;
                 coder.opts[offset as usize].prev_2 = false;
             }
         }
     }
 
     let mut start_len = 2;
 
     for rep_index in 0..REPS {
         let buf_back = &mf.buffer[mf.mf_ptr(1).wrapping_sub(reps[rep_index] as usize + 1)..];
         // if not_equal_16!(buf, buf_back) {
         //     continue;
         // }
         if buf[0] != buf_back[0] || buf[1] != buf_back[1] {
             continue;
         }
 
         let mut len_test = lzma_memcmplen(buf, buf_back, 2, buf_avail);
 
         while len_end < cur + len_test {
             coder.opts[(len_end + 1) as usize].price = RC_INFINITY_PRICE;
             len_end += 1;
         }
 
         let len_test_temp = len_test;
         let price = rep_match_price + get_pure_rep_price(coder, rep_index as u32, state, pos_state);
 
         while len_test >= 2 {
             let cur_and_len_price =
                 price + get_len_price(&coder.rep_len_encoder, len_test, pos_state);
 
             if cur_and_len_price < coder.opts[(cur + len_test) as usize].price {
                 coder.opts[(cur + len_test) as usize].price = cur_and_len_price;
                 coder.opts[(cur + len_test) as usize].pos_prev = cur;
                 coder.opts[(cur + len_test) as usize].back_prev = rep_index as u32;
                 coder.opts[(cur + len_test) as usize].prev_1_is_literal = false;
             }
             len_test -= 1;
         }
 
         len_test = len_test_temp;
 
         if rep_index == 0 {
             start_len = len_test + 1;
         }
 
         let mut len_test_2 = len_test + 1;
         let limit = my_min(buf_avail_full, len_test_2 + nice_len);
 
         if len_test_2 < limit {
             len_test_2 = lzma_memcmplen(buf, buf_back, len_test_2, limit);
         }
 
         len_test_2 -= len_test + 1;
 
         if len_test_2 >= 2 {
             let mut state_2 = state;
             update_long_rep(&mut state_2);
 
             let mut pos_state_next = (position + len_test) & coder.pos_mask;
 
             let cur_and_len_literal_price = price
                 + get_len_price(&coder.rep_len_encoder, len_test, pos_state)
                 + rc_bit_0_price(
                     *coder.is_match[state_2 as usize][pos_state_next as usize]
                         .lock()
                         .unwrap(),
                 )
                 + get_literal_price(
                     coder,
                     position + len_test,
                     buf[len_test as usize - 1] as u32,
                     true,
                     buf_back[len_test as usize] as u32,
                     buf[len_test as usize] as u32,
                 );
 
             state_2 = update_literal(state_2);
 
             pos_state_next = (position + len_test + 1) & coder.pos_mask;
 
             let next_rep_match_price = cur_and_len_literal_price
                 + rc_bit_1_price(
                     *coder.is_match[state_2 as usize][pos_state_next as usize]
                         .lock()
                         .unwrap(),
                 )
                 + rc_bit_1_price(*coder.is_rep[state_2 as usize].lock().unwrap());
 
             let offset = cur + len_test + 1 + len_test_2;
 
             while len_end < offset {
                 coder.opts[(len_end + 1) as usize].price = RC_INFINITY_PRICE;
                 len_end += 1;
             }
 
             let cur_and_len_price =
                 next_rep_match_price + get_rep_price(coder, 0, len_test_2, state_2, pos_state_next);
 
             if cur_and_len_price < coder.opts[offset as usize].price {
                 coder.opts[offset as usize].price = cur_and_len_price;
                 coder.opts[offset as usize].pos_prev = cur + len_test + 1;
                 coder.opts[offset as usize].back_prev = 0;
                 coder.opts[offset as usize].prev_1_is_literal = true;
                 coder.opts[offset as usize].prev_2 = true;
                 coder.opts[offset as usize].pos_prev_2 = cur;
                 coder.opts[offset as usize].back_prev_2 = rep_index as u32;
             }
         }
     }
 
     if new_len > buf_avail {
         new_len = buf_avail;
 
         matches_count = 0;
         while new_len > coder.matches[matches_count as usize].len {
             matches_count += 1;
         }
 
         coder.matches[matches_count as usize].len = new_len;
         matches_count += 1;
     }
 
     if new_len >= start_len {
         let normal_match_price =
             match_price + rc_bit_0_price(*coder.is_rep[state as usize].lock().unwrap());
 
         while len_end < cur + new_len {
             coder.opts[(len_end + 1) as usize].price = RC_INFINITY_PRICE;
             len_end += 1;
         }
 
         let mut i = 0;
         while start_len > coder.matches[i].len {
             i += 1;
         }
 
         for len_test in start_len.. {
             let cur_back = coder.matches[i].dist;
             let mut cur_and_len_price =
                 normal_match_price + get_dist_len_price(coder, cur_back, len_test, pos_state);
 
             if cur_and_len_price < coder.opts[(cur + len_test) as usize].price {
                 coder.opts[(cur + len_test) as usize].price = cur_and_len_price;
                 coder.opts[(cur + len_test) as usize].pos_prev = cur;
                 coder.opts[(cur + len_test) as usize].back_prev = cur_back + REPS as u32;
                 coder.opts[(cur + len_test) as usize].prev_1_is_literal = false;
             }
 
             if len_test == coder.matches[i].len {
                 let buf_back = &mf.buffer[mf.mf_ptr(1).wrapping_sub(cur_back as usize + 1)..];
                 let mut len_test_2 = len_test + 1;
                 let limit = my_min(buf_avail_full, len_test_2 + nice_len);
 
                 if len_test_2 < limit {
                     len_test_2 = lzma_memcmplen(buf, buf_back, len_test_2, limit);
                 }
 
                 len_test_2 -= len_test + 1;
 
                 if len_test_2 >= 2 {
                     let mut state_2 = state;
                     update_match(&mut state_2);
                     let mut pos_state_next = (position + len_test) & coder.pos_mask;
 
                     let cur_and_len_literal_price = cur_and_len_price
                         + rc_bit_0_price(
                             *coder.is_match[state_2 as usize][pos_state_next as usize]
                                 .lock()
                                 .unwrap(),
                         )
                         + get_literal_price(
                             coder,
                             position + len_test,
                             buf[len_test as usize - 1] as u32,
                             true,
                             buf_back[len_test as usize] as u32,
                             buf[len_test as usize] as u32,
                         );
 
                     state_2 = update_literal(state_2);
                     pos_state_next = (pos_state_next + 1) & coder.pos_mask;
 
                     let next_rep_match_price = cur_and_len_literal_price
                         + rc_bit_1_price(
                             *coder.is_match[state_2 as usize][pos_state_next as usize]
                                 .lock()
                                 .unwrap(),
                         )
                         + rc_bit_1_price(*coder.is_rep[state_2 as usize].lock().unwrap());
 
                     let offset = cur + len_test + 1 + len_test_2;
 
                     while len_end < offset {
                         coder.opts[(len_end + 1) as usize].price = RC_INFINITY_PRICE;
                         len_end += 1;
                     }
 
                     cur_and_len_price = next_rep_match_price
                         + get_rep_price(coder, 0, len_test_2, state_2, pos_state_next);
 
                     if cur_and_len_price < coder.opts[offset as usize].price {
                         coder.opts[offset as usize].price = cur_and_len_price;
                         coder.opts[offset as usize].pos_prev = cur + len_test + 1;
                         coder.opts[offset as usize].back_prev = 0;
                         coder.opts[offset as usize].prev_1_is_literal = true;
                         coder.opts[offset as usize].prev_2 = true;
                         coder.opts[offset as usize].pos_prev_2 = cur;
                         coder.opts[offset as usize].back_prev_2 = cur_back + REPS as u32;
                     }
                 }
 
                 if i + 1 == matches_count as usize {
                     break;
                 }
                 i += 1;
             }
         }
     }
 
     len_end
 }
 
 pub fn lzma_lzma_optimum_normal(
     coder: &mut LzmaLzma1Encoder,
     mf: &mut LzmaMf,
     back_res: &mut u32,
     len_res: &mut u32,
     position: u32,
 ) {
     // If we have symbols pending, return the next pending symbol.
     if coder.opts_end_index != coder.opts_current_index {
         assert!(mf.read_ahead > 0);
         *len_res =
             coder.opts[coder.opts_current_index as usize].pos_prev - coder.opts_current_index;
         *back_res = coder.opts[coder.opts_current_index as usize].back_prev;
         coder.opts_current_index = coder.opts[coder.opts_current_index as usize].pos_prev;
         return;
     }
 
     // Update the price tables. In LZMA SDK <= 4.60 (and possibly later)
     // this was done in both initialization function and in the main loop.
     // In liblzma they were moved into this single place.
     if mf.read_ahead == 0 {
         if coder.match_price_count >= (1 << 7) {
             fill_dist_prices(coder);
         }
 
         if coder.align_price_count >= ALIGN_SIZE as u32 {
             fill_align_prices(coder);
         }
     }
 
     // TODO: This needs quite a bit of cleaning still. But splitting
     // the original function into two pieces makes it at least a little
     // more readable, since those two parts don't share many variables.
     let mut len_end = helper1(coder, mf, back_res, len_res, position);
     if len_end == u32::MAX {
         return;
     }
 
     let mut reps = [0; REPS];
     reps.copy_from_slice(&coder.reps);
 
     let mut cur = 1;
     while cur < len_end {
         assert!(cur < OPTS as u32);
 
         coder.longest_match_length = lzma_mf_find(mf, &mut coder.matches_count, &mut coder.matches);
 
         if coder.longest_match_length >= mf.nice_len {
             break;
         }
 
         len_end = helper2(
             coder,
             &mut reps,
             mf,
             len_end,
             position + cur,
             cur,
             std::cmp::min(mf_avail(mf) + 1, OPTS as u32 - 1 - cur),
         );
         cur += 1;
     }
 
     backward(coder, len_res, back_res, cur);
     return;
 }