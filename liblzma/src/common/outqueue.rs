/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

//该文件都是和多线程相关的函数，暂时忽略

// use crate::{
//     api::{LzmaAllocator, LzmaRet, LzmaVli},
//     common::LZMA_THREADS_MAX,
// };

// use super::{lzma_bufcpy, };

// #[derive(Debug)]
// enum WorkerType<'a> {
//     Wtd(WorkerThread<'a>),

// }

// impl<'a> Default for WorkerType<'a> {
//     fn default() -> Self {
//         Self::Wtd(WorkerThread::default())
//     }
// }

// /// 输出缓冲区结构体
// #[derive(Debug, Default)]
// pub struct LzmaOutbuf<'a> {
//     /// 指向下一个缓冲区的指针。用于缓存缓冲区。
//     /// 工作线程不得修改此指针。
//     pub next: Option<&'a mut LzmaOutbuf<'a>>,

//     /// 由 lzma_outq_get_buf() 初始化，并由 lzma_outq_enable_partial_output() 使用。
//     /// 工作线程不得修改此指针。
//     pub worker: WorkerType<'a>,

//     /// 为 buf[] 分配的内存量。
//     /// 工作线程不得修改此值。
//     pub allocated: usize,

//     /// 工作线程中的写入位置，或者换句话说，写入 buf[] 的已完成数据量，可以复制输出。
//     ///
//     /// \note 另一个线程读取此变量，因此访问此变量需要互斥锁。
//     pub pos: usize,

//     /// 解压缩：工作线程中与上面输出 "pos" 匹配的输入缓冲区中的位置。
//     /// 用于检测是否可能从工作线程输出更多数据：如果它已消耗所有输入，则无法输出更多数据。
//     ///
//     /// \note 另一个线程读取此变量，因此访问此变量需要互斥锁。
//     pub decoder_in_pos: usize,

//     /// 当不再向此缓冲区写入数据时为 true。
//     ///
//     /// \note 另一个线程读取此变量，因此访问此变量需要互斥锁。
//     pub finished: bool,

//     /// 当从已完成的缓冲区读取最后一个字节时，lzma_outq_read() 的返回值。
//     /// 默认为 LZMA_STREAM_END。此值不得为 LZMA_OK。
//     /// 目的是允许解码器将错误代码传递给主线程，在此处设置代码，并将 finished 设置为 true。
//     pub finish_ret: LzmaRet,

//     /// 附加的大小信息。当 "finished" 为 true 时，lzma_outq_read() 可以读取这些信息。
//     pub unpadded_size: LzmaVli,
//     pub uncompressed_size: LzmaVli,

//     /// "allocated" 字节的缓冲区
//     pub buf: Vec<u8>,
// }

// /// 输出队列结构体
// #[derive(Debug, Default)]
// pub struct LzmaOutq<'a> {
//     /// 使用中的缓冲区链表。下一个输出字节将从头部读取，缓冲区将附加到尾部。
//     /// tail->next 始终为 NULL。
//     pub head: Option<&'a mut LzmaOutbuf<'a>>,
//     pub tail: Option<&'a mut LzmaOutbuf<'a>>,

//     /// 从 head->buf[] 中读取的字节数
//     pub read_pos: usize,

//     /// 当前未使用的已分配缓冲区链表。
//     /// 这样可以重用大小相似的缓冲区，而不需要每次重新分配。
//     /// 为简单起见，链表中的所有缓存缓冲区具有相同的分配大小。
//     pub cache: Option<&'a mut LzmaOutbuf<'a>>,

//     /// 为缓冲区分配的总内存量
//     pub mem_allocated: u64,

//     /// 在 head...tail 链表中使用的缓冲区所使用的内存量。
//     pub mem_in_use: u64,

//     /// 在 head...tail 链表中使用的缓冲区数量。
//     /// 当且仅当此值为零时，上面的 head 和 tail 指针为 NULL。
//     pub bufs_in_use: u32,

//     /// 已分配的缓冲区数量（使用中 + 缓存）
//     pub bufs_allocated: u32,

//     /// 允许分配的最大缓冲区数量
//     pub bufs_limit: u32,
// }

// #[inline]
// pub fn lzma_outq_has_buf(outq: &LzmaOutq) -> bool {
//     outq.bufs_in_use < outq.bufs_limit
// }

// /// 测试队列是否完全为空
// #[inline]
// pub fn lzma_outq_is_empty(outq: &LzmaOutq) -> bool {
//     outq.bufs_in_use == 0
// }

// /// 获取单个 lzma_outbuf 所需的内存量
// ///
// /// # 注意
// /// 调用者必须检查参数是否显著小于 usize::MAX 以避免整数溢出！
// #[inline]
// pub fn lzma_outq_outbuf_memusage(buf_size: usize) -> u64 {
//     assert!(buf_size <= usize::MAX - std::mem::size_of::<LzmaOutbuf>());
//     (std::mem::size_of::<LzmaOutbuf>() + buf_size) as u64
// }

// #[macro_export]
// macro_rules! GET_BUFS_LIMIT {
//     ($threads:expr) => {
//         2 * ($threads as u64)
//     };
// }

// /// 计算输出队列的内存使用量
// pub fn lzma_outq_memusage(buf_size_max: u64, threads: u32) -> u64 {
//     // 这有助于整数溢出检查：我们最多可以分配 GET_BUFS_LIMIT(LZMA_THREADS_MAX) 个缓冲区，
//     // 并且我们还需要一些额外的内存用于其他数据结构（即 /2）。
//     //
//     // lzma_outq_prealloc_buf() 仍然会接受比这更大的缓冲区。
//     const LIMIT: u64 = u64::MAX / GET_BUFS_LIMIT!(LZMA_THREADS_MAX) / 2;

//     if threads > LZMA_THREADS_MAX || buf_size_max > LIMIT {
//         return u64::MAX;
//     }

//     GET_BUFS_LIMIT!(threads) * lzma_outq_outbuf_memusage(buf_size_max.try_into().unwrap())
// }

// /// 将队列头移动到缓存
// fn move_head_to_cache(outq: &mut LzmaOutq, allocator: &LzmaAllocator) {
//     assert!(outq.head.is_some());
//     assert!(outq.tail.is_some());
//     assert!(outq.bufs_in_use > 0);

//     if let Some(mut buf) = outq.head.take() {
//         outq.head = buf.next.take();
//         if outq.head.is_none() {
//             outq.tail = None;
//         }

//         if let Some(cache) = &outq.cache {
//             if cache.allocated != buf.allocated {
//                 lzma_outq_clear_cache(outq, allocator);
//             }
//         }

//         buf.next = outq.cache.take();
//         outq.cache = Some(buf);

//         outq.bufs_in_use -= 1;
//         outq.mem_in_use -= lzma_outq_outbuf_memusage(buf.allocated);
//     }
// }

// /// 释放一个缓存的缓冲区
// fn free_one_cached_buffer(outq: &mut LzmaOutq, allocator: &LzmaAllocator) {
//     assert!(outq.cache.is_some());

//     if let Some(mut buf) = outq.cache.take() {
//         outq.cache = buf.next.take();

//         outq.bufs_allocated -= 1;
//         outq.mem_allocated -= lzma_outq_outbuf_memusage(buf.allocated);

//         // let mut tmp: Box<dyn std::any::Any> = Box::new(buf);
//         // lzma_free(&mut Some(&mut tmp), Some(allocator));
//     }
// }

// /// 清空缓存
// pub fn lzma_outq_clear_cache(outq: &mut LzmaOutq, allocator: &LzmaAllocator) {
//     while outq.cache.is_some() {
//         free_one_cached_buffer(outq, allocator);
//     }
// }

// /// 清空缓存，保留指定大小的缓冲区
// pub fn lzma_outq_clear_cache2(outq: &mut LzmaOutq, allocator: &LzmaAllocator, keep_size: usize) {
//     if outq.cache.is_none() {
//         return;
//     }

//     // 释放所有但保留一个
//     while outq.cache.as_ref().unwrap().next.is_some() {
//         free_one_cached_buffer(outq, allocator);
//     }

//     // 如果最后一个的大小不等于 keep_size，则释放它
//     if outq.cache.as_ref().unwrap().allocated != keep_size {
//         free_one_cached_buffer(outq, allocator);
//     }
// }

// /// 初始化输出队列
// pub fn lzma_outq_init(outq: &mut LzmaOutq, allocator: &LzmaAllocator, threads: u32) -> LzmaRet {
//     if threads > LZMA_THREADS_MAX {
//         return LzmaRet::OptionsError;
//     }

//     let bufs_limit = GET_BUFS_LIMIT!(threads);

//     // 清空 head/tail
//     while outq.head.is_some() {
//         move_head_to_cache(outq, allocator);
//     }

//     // 如果新的 buf_limit 小于旧的，我们可能需要释放一些缓存的缓冲区
//     while bufs_limit < outq.bufs_allocated.into() {
//         free_one_cached_buffer(outq, allocator);
//     }

//     outq.bufs_limit = bufs_limit as u32;
//     outq.read_pos = 0;

//     LzmaRet::Ok
// }

// /// 结束输出队列，释放所有资源
// pub fn lzma_outq_end(outq: &mut LzmaOutq, allocator: &LzmaAllocator) {
//     while outq.head.is_some() {
//         move_head_to_cache(outq, allocator);
//     }

//     lzma_outq_clear_cache(outq, allocator);
// }

// /// 预分配缓冲区
// pub fn lzma_outq_prealloc_buf(
//     outq: &mut LzmaOutq,
//     allocator: &LzmaAllocator,
//     size: usize,
// ) -> LzmaRet {
//     // 调用者必须通过 lzma_outq_has_buf() 检查
//     assert!(outq.bufs_in_use < outq.bufs_limit);

//     // 如果缓存中已经有适当大小的缓冲区，则无需操作
//     if let Some(cache) = &outq.cache {
//         if cache.allocated == size {
//             return LzmaRet::Ok;
//         }
//     }

//     // 检查大小是否会导致溢出
//     if size > usize::MAX - std::mem::size_of::<LzmaOutbuf>() {
//         return LzmaRet::MemError;
//     }

//     let alloc_size = lzma_outq_outbuf_memusage(size);

//     // 缓存可能有缓冲区，但大小不对
//     lzma_outq_clear_cache(outq, allocator);

//     // outq.cache = lzma_alloc(alloc_size as usize, Some(allocator)).downcast_mut();
//     if outq.cache.is_none() {
//         return LzmaRet::MemError;
//     }

//     outq.cache.as_mut().unwrap().next = None;

//     outq.bufs_allocated += size as u32;
//     outq.bufs_allocated += 1;
//     outq.mem_allocated += alloc_size;

//     LzmaRet::Ok
// }

// /// 获取缓冲区
// /// 改函数只在多线程中使用，暂时未实现多线程，先注释掉改函数
// // pub fn lzma_outq_get_buf<'a>(
// //     outq: &'a mut LzmaOutq<'a>,
// //     mut worker: Box<dyn std::any::Any>,
// // ) -> Option<&'a mut LzmaOutbuf<'a>> {
// //     // 调用者必须使用 lzma_outq_prealloc_buf() 确保这些条件
// //     assert!(outq.bufs_in_use < outq.bufs_limit);
// //     assert!(outq.bufs_in_use < outq.bufs_allocated);
// //     assert!(outq.cache.is_some());

// //     if let Some(mut buf) = outq.cache.take() {
// //         outq.cache = buf.next.take();

// //         if let Some(tail) = &mut outq.tail {
// //             assert!(outq.head.is_some());
// //             tail.next = Some(buf);
// //         } else {
// //             assert!(outq.head.is_none());
// //             outq.head = Some(buf);
// //         }

// //         outq.tail = Some(buf);

// //         // 初始化缓冲区
// //         if let Some(buf_ref) = outq.tail.as_mut() {
// //             buf_ref.worker = &mut worker;
// //             buf_ref.finished = false;
// //             buf_ref.finish_ret = LzmaRet::StreamEnd;
// //             buf_ref.pos = 0;
// //             buf_ref.decoder_in_pos = 0;
// //             buf_ref.unpadded_size = 0;
// //             buf_ref.uncompressed_size = 0;

// //             outq.bufs_in_use += 1;
// //             outq.mem_in_use += lzma_outq_outbuf_memusage(buf_ref.allocated);

// //             Some(buf_ref)
// //         } else {
// //             None
// //         }
// //     } else {
// //         None
// //     }
// // }

// /// 检查输出队列是否可读
// pub fn lzma_outq_is_readable(outq: &LzmaOutq) -> bool {
//     if outq.head.is_none() {
//         return false;
//     }

//     let head = outq.head.as_ref().unwrap();
//     outq.read_pos < head.pos || head.finished
// }

// /// 从输出队列中读取数据
// pub fn lzma_outq_read(
//     outq: &mut LzmaOutq,
//     allocator: &LzmaAllocator,
//     out: &mut Vec<u8>,
//     out_pos: &mut usize,
//     out_size: usize,
//     unpadded_size: Option<&mut LzmaVli>,
//     uncompressed_size: Option<&mut LzmaVli>,
// ) -> LzmaRet {
//     // 必须至少有一个缓冲区可供读取
//     if outq.bufs_in_use == 0 {
//         return LzmaRet::Ok;
//     }

//     // 获取缓冲区
//     let buf = outq.head.as_ref().unwrap();

//     // 从缓冲区复制到输出
//     lzma_bufcpy(
//         &mut buf.buf,
//         &mut outq.read_pos,
//         buf.pos,
//         out,
//         out_pos,
//         out_size,
//     );

//     // 如果没有从缓冲区获取所有数据，则返回
//     if !buf.finished || outq.read_pos < buf.pos {
//         return LzmaRet::Ok;
//     }

//     // 缓冲区已完成。告知调用者其大小信息
//     if let Some(unpadded_size) = unpadded_size {
//         *unpadded_size = buf.unpadded_size;
//     }

//     if let Some(uncompressed_size) = uncompressed_size {
//         *uncompressed_size = buf.uncompressed_size;
//     }

//     // 记住返回值
//     let finish_ret = buf.finish_ret;

//     // 释放此缓冲区以供进一步使用
//     move_head_to_cache(outq, allocator);
//     outq.read_pos = 0;

//     finish_ret
// }

// /// 启用部分输出
// pub fn lzma_outq_enable_partial_output(
//     outq: &mut LzmaOutq,
//     enable_partial_output: fn(&mut Box<dyn std::any::Any>),
// ) {
//     if outq.head.is_some()
//         && outq.head.unwrap().finished
//         && Some(outq.head.unwrap().worker).is_some()
//     {
//         enable_partial_output(outq.head.unwrap().worker);
//         outq.head.unwrap().worker = &mut (Box::new(()) as Box<dyn std::any::Any>);
//     }
// }
