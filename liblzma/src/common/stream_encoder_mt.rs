/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

// use std::sync::{Mutex, Condvar, Arc};
// use std::thread::JoinHandle;

// use common::{mythread_condtime_set, MyThreadCondTime};

// use crate::api::{LzmaAction, LzmaAllocator, LzmaBlock, LzmaFilter, LzmaMt, LzmaRet, LzmaStreamFlags, LzmaVli, LZMA_CHECK_ID_MAX, LZMA_VLI_UNKNOWN};
// use crate::check::lzma_check_is_supported;
// use crate::common::{lzma_block_encoder_init, lzma_block_header_encode, lzma_block_header_size, lzma_block_uncomp_encode, lzma_block_unpadded_size, lzma_index_init, lzma_mt_block_size, lzma_outq_init, lzma_stream_header_encode};

// use super::{lzma_block_buffer_bound64, lzma_bufcpy, lzma_easy_preset, lzma_end, lzma_filters_copy, lzma_filters_free, lzma_index_append, lzma_index_encoder_init, lzma_index_end, lzma_index_size, lzma_next_end, lzma_outq_end, lzma_outq_get_buf, lzma_outq_has_buf, lzma_outq_is_empty, lzma_outq_is_readable, lzma_outq_memusage, lzma_outq_prealloc_buf, lzma_outq_read, lzma_raw_encoder_memusage, lzma_stream_footer_encode, lzma_strm_init, LzmaIndex, LzmaNextCoder, LzmaOptionsEasy, LzmaOutbuf, LzmaOutq, LZMA_MEMUSAGE_BASE, LZMA_THREADS_MAX};

// // 假定以下常量已在其他地方定义
// pub const LZMA_FILTERS_MAX: usize = 16;
// pub const LZMA_STREAM_HEADER_SIZE: usize = 12;

// // 工作线程状态，对应 C 代码中的 WorkState
// #[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord,)]
// pub enum WorkState {
//     /// 等待处理工作
//     ThrIdle,
//     /// 正在编码
//     ThrRun,
//     /// 编码进行中但不再读取输入数据
//     ThrFinish,
//     /// 主线程要求线程停止当前工作但不退出
//     ThrStop,
//     /// 主线程要求线程退出
//     ThrExit,
// }

// // 重构后的 WorkerThread 结构体，对应 C 中的 struct WorkerThread_s
// #[derive(Debug)]
// pub struct WorkerThread<'a> {
//     /// 线程状态
//     pub state: WorkState,
//     /// 输入缓冲区，存放 coder->block_size 字节数据
//     pub r#in: Option<Vec<u8>>,
//     /// 输入缓冲区内可用数据量，仅由主线程修改
//     pub in_size: usize,
//     /// 输出缓冲区指针，每次新 Block 开始时由主线程设置
//     pub outbuf: Option<Arc<Mutex<LzmaOutbuf<'a>>>>,
//     /// 指向主结构体 LzmaStreamEncoderMt
//     pub coder: Option<Arc<Mutex<LzmaStreamEncoderMt<'a>>>>,
//     /// 内存分配器指针，由主线程设置，调用 lzma_end() 前不得更改
//     pub allocator: Option<Arc<LzmaAllocator>>,
//     /// 已压缩（未压缩）数据量，供进度统计使用
//     pub progress_in: u64,
//     /// 已就绪的压缩数据量，供进度统计使用
//     pub progress_out: u64,
//     /// Block 编码器
//     pub block_encoder: LzmaNextCoder<'a>,
//     /// 编码该 Block 的选项
//     pub block_options: LzmaBlock,
//     /// 本线程使用的过滤器链数组，大小为 LZMA_FILTERS_MAX + 1
//     pub filters: [LzmaFilter; LZMA_FILTERS_MAX + 1],
//     /// 指向空闲线程链中下一个结构
//     pub next: Option<Arc<Mutex<WorkerThread<'a>>>>,
//     /// 线程互斥锁
//     pub mutex: Mutex<()>,
//     /// 条件变量
//     pub cond: Condvar,
//     /// 线程 ID，用于 join 线程；Rust 中使用 JoinHandle 替代
//     pub thread_id: Option<JoinHandle<()>>,
// }

// // impl<'a> Clone for WorkerThread<'a> {
// //     fn clone(&self) -> Self {
// //         WorkerThread {
// //             // 直接拷贝或调用 clone
// //             state: self.state,
// //             r#in: self.r#in.clone(),
// //             in_size: self.in_size,
// //             outbuf: self.outbuf.clone(),
// //             coder: self.coder.clone(),
// //             allocator: self.allocator.clone(),
// //             progress_in: self.progress_in,
// //             progress_out: self.progress_out,
// //             block_encoder: self.block_encoder.clone(),
// //             block_options: self.block_options.clone(),
// //             filters: self.filters.clone(),
// //             next: self.next.clone(),
// //             // 新建一个 mutex 和 condvar，线程句柄不复制
// //             mutex: Mutex::new(()),
// //             cond: Condvar::new(),
// //             thread_id: None,
// //         }
// //     }
// // }

// // 重构后的 LzmaStreamEncoderMt 结构体，对应 C 中的 struct lzma_stream_coder_s
// #[derive(Debug)]
// pub struct LzmaStreamEncoderMt<'a> {
//     /// 当前编码阶段状态：SEQ_STREAM_HEADER, SEQBLOCK, SEQINDEX, SEQSTREAMFOOTER
//     pub sequence: StreamSequence,
//     /// 每当输入达到 block_size 字节时便开始新 Block，除非提前使用了 FullFlush 或 FullBarrier
//     pub block_size: usize,
//     /// 下一 Block 使用的过滤器链数组
//     pub filters: [LzmaFilter; LZMA_FILTERS_MAX + 1],
//     /// 缓存的过滤器链副本，用于复用等待空闲线程时的过滤器设置，更新 filters 后清空此缓存
//     pub filters_cache: [LzmaFilter; LZMA_FILTERS_MAX + 1],
//     /// 用于保存 Block 大小的 Index 指针
//     pub index: Option<Box<LzmaIndex>>,
//     /// Index 编码器
//     pub index_encoder: LzmaNextCoder<'a>,
//     /// 流头与流尾使用的 Stream Flags
//     pub stream_flags: LzmaStreamFlags,
//     /// 存储 Stream Header 与 Stream Footer 的缓冲区
//     pub header: [u8; LZMA_STREAM_HEADER_SIZE],
//     /// header 缓冲区当前读取位置
//     pub header_pos: usize,
//     /// 多线程编码生成的输出队列
//     pub outq: LzmaOutq<'a>,
//     /// 为每个 lzma_outbuf.buf 分配的内存大小
//     pub outbuf_alloc_size: usize,
//     /// 当无法充分利用输入且输出缓冲区无法填满时的最长等待时间（毫秒）
//     pub timeout: u32,
//     /// 工作线程返回的错误码
//     pub thread_error: LzmaRet,
//     /// 动态分配的所有工作线程结构体数组
//     pub threads: Option<Vec<Arc<Mutex<WorkerThread<'a>>>>>,
//     /// 最大可创建工作线程数
//     pub threads_max: u32,
//     /// 已初始化的工作线程数量
//     pub threads_initialized: u32,
//     /// 空闲线程栈，工作线程结束后会放回此栈；初始为空
//     pub threads_free: Option<Arc<Mutex<WorkerThread<'a>>>>,
//     /// 主线程写入新输入数据时最近使用的工作线程
//     pub thr: Option<Arc<Mutex<WorkerThread<'a>>>>,
//     /// 已完成 Block 的未压缩数据总量
//     pub progress_in: u64,
//     /// 已完成的压缩数据总量（包括 Stream Header 与 Blocks）
//     pub progress_out: u64,
//     /// 保护共享数据的互斥锁
//     pub mutex: Mutex<()>,
//     /// 条件变量
//     pub cond: Condvar,
// }

// /// 枚举 LzmaStreamEncoderMt 中的 sequence 状态
// #[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd)]
// pub enum StreamSequence {
//     SEQSTREAMHEADER,
//     SEQBLOCK,
//     SEQINDEX,
//     SEQSTREAMFOOTER,
// }

// // 为了与 C 代码保持一致，定义类型别名
// // pub type LzmaStreamEncoderMt = LzmaStreamEncoderMt;
// // pub type LzmaStreamEncoderMt = LzmaStreamEncoderMt; // 同义词，可根据需要调整

// pub fn worker_error(thr: &mut WorkerThread, ret: LzmaRet) {
//     // 确保 ret 非 LZMA_OK、非 LzmaRet::StreamEnd
//     assert!(ret != LzmaRet::Ok && ret != LzmaRet::StreamEnd);
//     // 锁定主解码器的互斥锁，更新 thread_error，并发出条件变量信号
//     if let Some(ref coder_arc) = thr.coder {
//         let mut coder = coder_arc.lock().unwrap();
//         if coder.thread_error == LzmaRet::Ok {
//             coder.thread_error = ret;
//         }
//         coder.cond.notify_one();
//     }
// }

// pub fn worker_encode(thr: &mut WorkerThread, out_pos: &mut usize, mut state: WorkState) -> WorkState {
//     // 检查工作线程进度计数必须为 0
//     assert!(thr.progress_in == 0);
//     assert!(thr.progress_out == 0);

//     // 设置 Block 选项，从主解码器中取相关配置
//     if let Some(ref coder_arc) = thr.coder {
//         let coder = coder_arc.lock().unwrap();
//         thr.block_options = LzmaBlock {
//             version: 0,
//             check: coder.stream_flags.check.clone(),
//             compressed_size: thr.outbuf.as_ref().unwrap().lock().unwrap().allocated as u64,
//             uncompressed_size: coder.block_size as u64,
//             filters: thr.filters.clone().to_vec(),
//             // 其它字段按需求填充
//             ..Default::default()
//         };
//     }
//     // 计算 Block Header 的最大尺寸
//     let ret = lzma_block_header_size(&mut thr.block_options);
//     if ret != LzmaRet::Ok {
//         worker_error(thr, ret);
//         return WorkState::ThrStop;
//     }
//     // 初始化 Block 编码器
//     let ret = lzma_block_encoder_init(&mut thr.block_encoder, thr.allocator.as_ref().unwrap(), &mut thr.block_options);
//     if ret != LzmaRet::Ok {
//         worker_error(thr, ret);
//         return WorkState::ThrStop;
//     }

//     let mut in_pos: usize = 0;
//     let mut in_size: usize = 0;

//     // 将编码结果的初始位置设为 Block Header 大小
//     *out_pos = thr.block_options.header_size as usize;
//     let out_size = thr.outbuf.as_ref().unwrap().lock().unwrap().allocated;

//     // 编码循环
//     loop {
//         {   // 使用工作线程自己的 mutex 保护共享数据
//             let mut guard = thr.mutex.lock().unwrap();
//             thr.progress_in = in_pos as u64;
//             thr.progress_out = *out_pos  as u64;
//             // 等待直到有新输入或状态变更
//             while in_size == thr.in_size && thr.state == WorkState::ThrRun {
//                 guard = thr.cond.wait(guard).unwrap();
//             }
//             state = thr.state;
//             in_size = thr.in_size;
//         }

//         // 如果被要求停止或退出，则返回当前状态
//         if let WorkState::ThrStop | WorkState::ThrExit | WorkState::ThrFinish = state {
//             if state >= WorkState::ThrStop {
//                 return state;
//             }
//         }

//         // 选择编码动作：如果状态为 ThrFinish 则使用 FINISH，否则 RUN
//         let mut action = if state == WorkState::ThrFinish { LzmaAction::Finish } else { LzmaAction::Run };

//         // 限制每次输入的最大数据量以快速响应主线程请求
//         const IN_CHUNK_MAX: usize = 16384;
//         let mut in_limit = in_size;
//         if in_size.saturating_sub(in_pos) > IN_CHUNK_MAX {
//             in_limit = in_pos + IN_CHUNK_MAX;
//             action = LzmaAction::Run;
//         }

//         let mut ret:LzmaRet = LzmaRet::Ok;
//         if let Some(code) = thr.block_encoder.code {
//             ret = code(
//                 thr.block_encoder.coder.as_mut().unwrap(),
//                 thr.allocator.as_ref().unwrap(),
//                 &thr.r#in.as_ref().unwrap().as_slice().to_vec(),
//                 &mut in_pos,
//                 in_limit,
//                 &mut thr.outbuf.as_ref().unwrap().lock().unwrap().buf,
//                 out_pos,
//                 out_size,
//                 action,
//             );
//         }

//         if ret != LzmaRet::Ok || *out_pos >= out_size {
//             // 退出循环
//             break;
//         }
//     }

//     // 分情况处理 ret
//     match ret {
//         LzmaRet::StreamEnd => {
//             assert!(state == WorkState::ThrFinish);
//             // 编码 Block Header，利用已经压缩完成的数值写入 Header 部分
//             let ret = lzma_block_header_encode(&mut thr.block_options, &mut thr.outbuf.as_ref().unwrap().lock().unwrap().buf);
//             if ret != LzmaRet::Ok {
//                 worker_error(thr, ret);
//                 return WorkState::ThrStop;
//             }
//         }
//         LzmaRet::Ok => {
//             // 数据不可压缩：等待接收所有输入
//             {
//                 let mut guard = thr.mutex.lock().unwrap();
//                 while thr.state == WorkState::ThrRun {
//                     guard = thr.cond.wait(guard).unwrap();
//                 }
//                 state = thr.state;
//                 in_size = thr.in_size;
//             }
//             if state >= WorkState::ThrStop {
//                 return state;
//             }
//             // 重置输出位置并调用无压缩编码
//             *out_pos = 0;
//             let ret = lzma_block_uncomp_encode(&mut thr.block_options,
//                                                thr.r#in.as_mut().unwrap(),
//                                                in_size,
//                                                &mut thr.outbuf.as_ref().unwrap().lock().unwrap().buf,
//                                                out_pos,
//                                                out_size);
//             if ret != LzmaRet::Ok {
//                 worker_error(thr, LzmaRet::ProgError);
//                 return WorkState::ThrStop;
//             }
//         }
//         _ => {
//             worker_error(thr, ret);
//             return WorkState::ThrStop;
//         }
//     }

//     // 设置输出缓冲区的尺寸信息供主线程写 Index 字段使用
//     {
//         let unpadded_size = lzma_block_unpadded_size(&thr.block_options);
//         assert!(unpadded_size != 0);
//         // 假定 outbuf 为可变引用
//         let mut outbuf = thr.outbuf.as_ref().unwrap().lock().unwrap();
//         outbuf.unpadded_size = unpadded_size;
//         outbuf.uncompressed_size = thr.block_options.uncompressed_size;
//     }

//     return WorkState::ThrFinish;
// }

// pub fn worker_start(thr_ptr: Arc<Mutex<WorkerThread>>) {
//     loop {
//         // 等待工作
//         {
//             // 锁定 thr->mutex
//             let mut thr = thr_ptr.lock().unwrap();
//             loop {
//                 // 如果状态为 ThrStop，则设置为 ThrIdle并通知等待者
//                 if thr.state == WorkState::ThrStop {
//                     thr.state = WorkState::ThrIdle;
//                     thr.cond.notify_one();
//                 }
//                 let state = thr.state;
//                 if state != WorkState::ThrIdle {
//                     break;
//                 }
//                 thr = thr.cond.wait(thr).unwrap();
//             }
//             // 保留当前状态到变量 state（解锁后使用）
//             let state = thr.state;
//             // out_pos 保存编码后的输出位置，初始化为 0
//             let mut out_pos:usize = 0;
//             // 这里暂时解锁 mutex 后再使用 worker_encode
//             drop(thr);

//             // 断言状态不为 ThrIdle 或 ThrStop
//             assert!(state != WorkState::ThrIdle && state != WorkState::ThrStop);

//             // 如果状态在 ThrRun 或 ThrFinish范围内，调用 worker_encode
//             let new_state = if state <= WorkState::ThrFinish {
//                 let mut thr = thr_ptr.lock().unwrap();
//                 // worker_encode 的签名类似于：
//                 // fn worker_encode(thr: &mut WorkerThread, out_pos: &mut usize, state: WorkState) -> WorkState
//                 worker_encode(&mut *thr, &mut out_pos, state)
//             } else {
//                 state
//             };

//             if new_state == WorkState::ThrExit {
//                 break;
//             }
//          // end 工作循环，thr_ptr 的 MutexGuard 已释放

//         // 将线程标记为 idle（非退出），通知等待者

//             let mut thr = thr_ptr.lock().unwrap();
//             if thr.state != WorkState::ThrExit {
//                 thr.state = WorkState::ThrIdle;
//                 thr.cond.notify_one();
//             }

//         // 更新主线程进度信息和归还线程到空闲栈

//             let coder_arc = {
//                 let thr = thr_ptr.lock().unwrap();
//                 thr.coder.clone().expect("coder should be set")
//             };
//             let mut coder = coder_arc.lock().unwrap();
//             {
//                 let mut thr = thr_ptr.lock().unwrap();
//                 // 如果编码结束，将输出缓冲区标记为 finished 并更新写入位置
//                 if new_state_eq(&thr.state, WorkState::ThrFinish) {
//                     if let Some(ref outbuf_arc) = thr.outbuf {
//                         let mut outbuf = outbuf_arc.lock().unwrap();
//                         // 此处 out_pos 来自 worker_encode 结果（假定已保存于 thr 内部）
//                         // 设置 finished 标记
//                         outbuf.pos = out_pos; // 假定 temp_out_pos 存储了编码后的长度
//                         outbuf.finished = true;
//                     }
//                 }
//                 // 更新主线程进度信息
//                 if let Some(ref outbuf_arc) = thr.outbuf {
//                     let outbuf = outbuf_arc.lock().unwrap();
//                     coder.progress_in += outbuf.uncompressed_size;
//                     coder.progress_out += out_pos as u64;
//                 }
//                 thr.progress_in = 0;
//                 thr.progress_out = 0;
//                 // 将 thr 返回到 coder 的空闲线程栈中
//                 thr.next = coder.threads_free.take();
//                 coder.threads_free = Some(thr_ptr.clone());
//             }
//             coder.cond.notify_one();
//         }
//     } // end loop

//     // 退出前，释放资源
//     {
//         let mut thr = thr_ptr.lock().unwrap();
//         // 释放线程特定过滤器选项
//         lzma_filters_free(&mut thr.filters, thr.allocator.as_ref().unwrap());
//     }
//     {
//         let mut thr = thr_ptr.lock().unwrap();
//         // 销毁互斥量与条件变量无需显式调用，Rust 会自动 drop
//     }
//     {
//         let mut thr = thr_ptr.lock().unwrap();
//         lzma_next_end(&mut thr.block_encoder, thr.allocator.as_ref().unwrap());
//         // 释放输入缓冲区
//         thr.r#in.take();
//     }
//     // 返回线程退出值
//     ()
// }

// // 辅助函数，用于比较 WorkState（由于枚举实现了 PartialOrd，可直接使用 <= 操作符）
// fn new_state_eq(state: &WorkState, cmp: WorkState) -> bool {
//     *state == cmp
// }

// // threads_stop 函数：让所有线程停止编码但不退出，可选等待所有线程达到 idle 状态
// pub fn threads_stop(coder: &mut LzmaStreamEncoderMt, wait_for_threads: bool) {
//     // 通知所有线程设置状态为 ThrStop
//     if let Some(ref threads) = coder.threads {
//         for thr_arc in threads.iter() {
//             let mut thr = thr_arc.lock().unwrap();
//             thr.state = WorkState::ThrStop;
//             thr.cond.notify_one();
//         }
//     }
//     if !wait_for_threads {
//         return;
//     }
//     // 等待所有线程达到 idle 状态
//     if let Some(ref threads) = coder.threads {
//         for thr_arc in threads.iter() {
//             let mut thr = thr_arc.lock().unwrap();
//             while thr.state != WorkState::ThrIdle {
//                 thr = thr.cond.wait(thr).unwrap();
//             }
//         }
//     }
// }

// // threads_end 函数：停止所有线程并释放相关资源，等待所有线程退出
// pub fn threads_end(coder: &mut LzmaStreamEncoderMt, allocator: &LzmaAllocator) {
//     if let Some(ref threads) = coder.threads {
//         for thr_arc in threads.iter() {
//             let mut thr = thr_arc.lock().unwrap();
//             thr.state = WorkState::ThrExit;
//             thr.cond.notify_one();
//         }
//         // 等待所有线程退出
//         if let Some(ref threads) = coder.threads {
//             for thr_arc in threads.iter() {
//                 let mut thr = thr_arc.lock().unwrap();
//                 if let Some(handle) = thr.thread_id.take() { // 使用 take() 移出 JoinHandle
//                     handle.join().expect("Thread join failed");
//                 }
//             }
//         }
//     }
//     // 释放线程结构体集合
//     coder.threads.take();
// }

// // initialize_new_thread：初始化新的工作线程结构体并创建新线程
// pub fn initialize_new_thread(coder: &mut LzmaStreamEncoderMt, allocator: &LzmaAllocator) -> LzmaRet {
//     // 假定 coder.threads 为 Vec<Arc<Mutex<WorkerThread>>> 已分配，且 coder.threads_initialized 为已初始化计数
//     if coder.threads.is_none() {
//         coder.threads = Some(Vec::with_capacity(coder.threads_max as usize));
//     }
//     let threads = coder.threads.as_mut().unwrap();
//     // 检查是否达到最大线程数
//     if coder.threads_initialized as u32 >= coder.threads_max {
//         return LzmaRet::OptionsError;
//     }
//     // 初始化新的 WorkerThread
//     let mut new_thr = WorkerThread {
//         state: WorkState::ThrIdle,
//         r#in: Some(vec![0; coder.block_size]),
//         in_size: 0,
//         outbuf: None,
//         coder: Some(Arc::new(Mutex::new(coder.clone()))), // 假定 clone() 实现了深拷贝或使用引用
//         allocator: Some(allocator.clone()),
//         progress_in: 0,
//         progress_out: 0,
//         block_encoder: LzmaNextCoder::default(), // 假定已定义默认值
//         block_options: Default::default(),    // 假定 LzmaBlock 实现 Default
//         filters: Default::default(),           // 默认数组，filters[0].id 设置后续使用
//         next: None,
//         mutex: Mutex::new(()),
//         cond: Condvar::new(),
//         thread_id: None,
//     };
//     new_thr.filters[0].id = LZMA_VLI_UNKNOWN; // 假定 LZMA_VLI_UNKNOWN 已定义
//     // 创建线程：克隆 Arc 包装 new_thr，并传入 worker_start 函数
//     let thr_arc = Arc::new(Mutex::new(new_thr));
//     let thr_arc_clone = thr_arc.clone();
//     let handle = std::thread::spawn(move || worker_start(thr_arc_clone));
//     {
//         let mut thr = thr_arc.lock().unwrap();
//         thr.thread_id = Some(handle);
//     }
//     threads.push(thr_arc.clone());
//     coder.threads_initialized += 1;
//     // 将 coder->thr 指向新创建的线程
//     coder.thr = Some(thr_arc);
//     LzmaRet::Ok
// }

// // get_thread 函数：获取一个工作线程，如果已有空闲线程则复用，否则新建线程
// pub fn get_thread(coder: &mut LzmaStreamEncoderMt, allocator: &LzmaAllocator) -> LzmaRet {
//     // 如果输出队列没有空闲缓冲区，则直接返回
//     if !lzma_outq_has_buf(&coder.outq) {
//         return LzmaRet::Ok;
//     }
//     // 预分配新的输出缓冲区，确保后续获取线程时有足够内存空间
//     return_if_error(lzma_outq_prealloc_buf(&mut coder.outq, allocator, coder.outbuf_alloc_size));
//     // 如果 filters_cache 未填充，则复制 filters 到 filters_cache
//     if coder.filters_cache[0].id == LZMA_VLI_UNKNOWN {
//         return_if_error(lzma_filters_copy(&coder.filters, &mut coder.filters_cache, allocator));
//     }
//     {
//         // 从 coder->mutex 保护的空闲线程栈中取出一个线程
//         let mut lock = coder.mutex.lock().unwrap();
//         if let Some(free_thr) = coder.threads_free.take() {
//             coder.thr = Some(free_thr);
//         }
//     }
//     if coder.thr.is_none() {
//         // 若线程未空闲且达到最大线程数，则返回
//         if coder.threads_initialized as u32 == coder.threads_max {
//             return LzmaRet::Ok;
//         }
//         // 否则初始化新线程
//         return_if_error(initialize_new_thread(coder, allocator));
//     }
//     // 重置获取到的线程状态
//     if let Some(ref thr_arc) = coder.thr {
//         let mut thr = thr_arc.lock().unwrap();
//         thr.state = WorkState::ThrRun;
//         thr.in_size = 0;
//         // 分配新的输出缓冲区给该线程
//         thr.outbuf = Some(lzma_outq_get_buf(&mut coder.outq, std::ptr::null_mut()));
//         // 释放旧的线程特定过滤器选项并用 filters_cache 替换，然后清空缓存
//         lzma_filters_free(&mut thr.filters, allocator);
//         thr.filters.copy_from_slice(&coder.filters_cache);
//         coder.filters_cache[0].id = LZMA_VLI_UNKNOWN;
//         thr.cond.notify_one();
//     }
//     LzmaRet::Ok
// }

// // 辅助宏转换为函数，返回错误码或 LzmaRet::Ok
// fn return_if_error(ret: LzmaRet) -> LzmaRet {
//     if ret != LzmaRet::Ok {
//         return ret;
//     }
//     LzmaRet::Ok
// }
// pub fn stream_encode_in(
//     coder: &mut LzmaStreamEncoderMt,
//     allocator: &LzmaAllocator,
//     input: &Vec<u8>,
//     in_pos: &mut usize,
//     in_size: usize,
//     action: LzmaAction,
// ) -> LzmaRet {
//     // 循环直到输入全部处理完或当前工作线程不存在且动作为 LZMA_RUN
//     while *in_pos < in_size || (coder.thr.is_some() && action != LzmaAction::Run) {
//         // 如工作线程不存在，尝试获取一个新线程
//         if coder.thr.is_none() {
//             let ret = get_thread(coder, allocator);
//             if coder.thr.is_none() {
//                 return ret;
//             }
//         }
//         // 假定 coder.thr 存在，现在复制输入数据到线程缓冲区
//         let mut thr_in_size = {
//             let thr = coder.thr.as_ref().unwrap().lock().unwrap();
//             thr.in_size
//         };
//         // 调用 lzma_bufcpy 将输入数据 from input 到 thr.in。
//         // 这里假定 lzma_bufcpy 函数签名为：
//         // fn lzma_bufcpy(src: &[u8], src_pos: &mut usize, src_size: usize,
//         //                dst: &mut Option<Vec<u8>>, dst_pos: &mut usize, dst_limit: usize);
//         lzma_bufcpy(input, in_pos, in_size, &mut coder.thr.as_ref().unwrap().lock().unwrap().r#in.unwrap(), &mut thr_in_size, coder.block_size);
//         // 判断是否应通知 Block 编码器完成工作:
//         // 如果线程输入大小达到 coder.block_size 或所有输入都已用完且 action != LZMA_RUN
//         let finish = thr_in_size == coder.block_size || (*in_pos == in_size && action != LzmaAction::Run);
//         let mut block_error = false;
//         {
//             // 锁定当前工作线程的互斥锁
//             let mut thr = coder.thr.as_ref().unwrap().lock().unwrap();
//             if thr.state == WorkState::ThrIdle {
//                 // 如果工作线程处于空闲状态，说明 Block 编码器出错
//                 block_error = true;
//             } else {
//                 // 向工作线程更新新输入数据大小
//                 thr.in_size = thr_in_size;
//                 if finish {
//                     thr.state = WorkState::ThrFinish;
//                 }
//                 thr.cond.notify_one();
//             }
//         }
//         if block_error {
//             let ret = {
//                 let coder_guard = coder.mutex.lock().unwrap();
//                 coder_guard.thread_error
//             };
//             return ret;
//         }
//         if finish {
//             coder.thr = None;
//         }
//     }
//     LzmaRet::Ok
// }

// /// 等待直到有更多输入、更多输出就绪、或达到超时。
// pub fn wait_for_work(
//     coder: &mut LzmaStreamEncoderMt,
//     wait_abs: &mut MyThreadCondTime,
//     has_blocked: &mut bool,
//     has_input: bool,
// ) -> bool {
//     // 如果设置了超时且尚未计算绝对等待时间，则进行设置
//     if coder.timeout != 0 && !*has_blocked {
//         *has_blocked = true;
//         mythread_condtime_set(wait_abs, &coder.cond, coder.timeout);
//     }
//     let mut timed_out = false;
//     {
//         let mut guard = coder.mutex.lock().unwrap();
//         // 循环直到满足至少以下一项条件：
//         //  - 如果有输入，则必须有空闲线程和输出缓冲区可用；
//         //  - 或输出队列中有可读数据；
//         //  - 或线程错误已发生；
//         //  - 或超时发生。
//         while ((!has_input || coder.threads_free.is_none() || !lzma_outq_has_buf(&coder.outq))
//             && !lzma_outq_is_readable(&coder.outq)
//             && coder.thread_error == LzmaRet::Ok
//             && !timed_out)
//         {
//             if coder.timeout != 0 {
//                 let (g, wait_result) = coder.cond.wait_timeout(guard, wait_abs.remaining_time()).unwrap();
//                 guard = g;
//                 if wait_result.timed_out() {
//                     timed_out = true;
//                 }
//             } else {
//                 guard = coder.cond.wait(guard).unwrap();
//             }
//         }
//     }
//     timed_out
// }

// /// 主流编码函数：负责将输入转换为压缩输出，并处理 Index 和 Stream Footer。
// pub fn stream_encode_mt(
//     coder_ptr: &mut LzmaStreamEncoderMt,
//     allocator: &LzmaAllocator,
//     input: &Vec<u8>,
//     in_pos: &mut usize,
//     in_size: usize,
//     out: &mut [u8],
//     out_pos: &mut usize,
//     out_size: usize,
//     action: LzmaAction,
// ) -> LzmaRet {
//     let coder = coder_ptr;
//     match coder.sequence {
//         StreamSequence::SEQSTREAMHEADER => {
//             // 将 Stream Header 从 coder.header 拷贝到 out[]
//             lzma_bufcpy(&coder.header.to_vec(), &mut coder.header_pos, coder.header.len(), &mut out.to_vec(), out_pos, out_size);
//             if coder.header_pos < coder.header.len() {
//                 return LzmaRet::Ok;
//             }
//             coder.header_pos = 0;
//             coder.sequence = StreamSequence::SEQBLOCK;
//             // “Fall through”到 SEQBLOCK
//         }
//         _ => {}
//     }
//     if coder.sequence == StreamSequence::SEQBLOCK {
//         // 初始化局部变量
//         let mut unpadded_size:LzmaVli = 0;
//         let mut uncompressed_size: LzmaVli = 0;
//         let mut ret: LzmaRet = LzmaRet::Ok;
//         let mut has_blocked = false;
//         let mut wait_abs = MyThreadCondTime::default();
//         'block_loop: loop {
//             {
//                 // 锁定 coder 的互斥锁以查询状态和尝试读取输出数据
//                 let mut guard = coder.mutex.lock().unwrap();
//                 ret = lzma_outq_read(&mut coder.outq, allocator, &mut out.to_vec(), out_pos, out_size, Some(&mut unpadded_size), Some(&mut uncompressed_size));
//                 // 检查 Block 编码器错误
//                 if coder.thread_error != LzmaRet::Ok {
//                     ret = coder.thread_error;
//                     break 'block_loop;
//                 }
//             }
//             if ret == LzmaRet::StreamEnd {
//                 // 当前 Block 编码完成，将 Index 更新
//                 ret = lzma_index_append(coder.index.as_mut().unwrap(), allocator, unpadded_size, uncompressed_size);
//                 if ret != LzmaRet::Ok {
//                     threads_stop(coder, false);
//                     return ret;
//                 }
//                 if *out_pos < out_size {
//                     continue;
//                 }
//             }
//             if ret != LzmaRet::Ok {
//                 threads_stop(coder, false);
//                 return ret;
//             }
//             // 尝试将更多未压缩数据提交给工作线程编码
//             ret = stream_encode_in(coder, allocator, &input.to_vec(), in_pos, in_size, action);
//             if ret != LzmaRet::Ok {
//                 threads_stop(coder, false);
//                 return ret;
//             }
//             // 如果所有输入数据已处理完
//             if *in_pos == in_size {
//                 if action == LzmaAction::Run {
//                     return LzmaRet::Ok;
//                 }
//                 if action == LzmaAction::FullBarrier {
//                     return LzmaRet::StreamEnd;
//                 }
//                 if lzma_outq_is_empty(&coder.outq) {
//                     if action == LzmaAction::Finish {
//                         break 'block_loop;
//                     }
//                     if action == LzmaAction::FullFlush {
//                         return LzmaRet::StreamEnd;
//                     }
//                 }
//             }
//             // 若输出缓冲区已满，则返回以便调用者扩充输出空间
//             if *out_pos == out_size {
//                 return LzmaRet::Ok;
//             }
//             // 否则等待，直到有工作可做
//             if wait_for_work(coder, &mut wait_abs, &mut has_blocked, *in_pos < in_size) {
//                 return LzmaRet::RetInternal1;
//             }
//         }
//         // 调用 Index 编码器初始化，准备编码 Index 字段
//         ret = lzma_index_encoder_init(&mut coder.index_encoder, allocator, coder.index.as_ref().unwrap());
//         if ret != LzmaRet::Ok {
//             return ret;
//         }
//         coder.sequence = StreamSequence::SEQINDEX;
//         // 更新进度信息，假定 lzma_index_size() 和 LZMA_STREAM_HEADER_SIZE 已定义
//         coder.progress_out += lzma_index_size(coder.index.as_ref().unwrap()) + LZMA_STREAM_HEADER_SIZE as u64;
//     }
//     if coder.sequence == StreamSequence::SEQINDEX {
//         let mut ret: LzmaRet = LzmaRet::Ok;
//         if let Some(code) = coder.index_encoder.code {
//             // 编码 Index 字段
//             ret = code(
//                 coder.index_encoder.coder.as_ref().unwrap().as_ref(),
//                 allocator,
//                 &[],
//                 &mut 0,
//                 0,
//                 out,
//                 out_pos,
//                 out_size,
//                 LzmaAction::Run,
//             );
//         }

//         if ret != LzmaRet::StreamEnd {
//             return ret;
//         }
//         // 将 Stream Footer 编码到 coder.header
//         coder.stream_flags.backward_size = lzma_index_size(coder.index.as_ref().unwrap());
//         if lzma_stream_footer_encode(&mut coder.stream_flags, &mut coder.header) != LzmaRet::Ok {
//             return LzmaRet::ProgError;
//         }
//         coder.sequence = StreamSequence::SEQSTREAMFOOTER;
//     }
//     if coder.sequence == StreamSequence::SEQSTREAMFOOTER {
//         lzma_bufcpy(&coder.header.to_vec(), &mut coder.header_pos, coder.header.len(), &mut out.to_vec(), out_pos, out_size);
//         return if coder.header_pos < coder.header.len() { LzmaRet::Ok } else { LzmaRet::StreamEnd };
//     }
//     // 理论上不可能达到这里
//     LzmaRet::ProgError
// }

// // stream_encoder_mt_end: 结束多线程流编码器并释放相关资源。
// pub fn stream_encoder_mt_end(coder_ptr: Box<LzmaStreamEncoderMt>, allocator: &LzmaAllocator) {
//     // 注意：先结束所有线程，然后释放输出队列、过滤链、索引编码器、Index 以及销毁条件变量与互斥锁。
//     let mut coder = coder_ptr;
//     threads_end(&mut coder, allocator);
//     lzma_outq_end(&mut coder.outq, allocator);
//     lzma_filters_free(&mut coder.filters, allocator);
//     lzma_filters_free(&mut coder.filters_cache, allocator);
//     lzma_next_end(&mut coder.index_encoder, allocator);
//     lzma_index_end(&mut coder.index.take().unwrap(), allocator);
//     // 销毁条件变量与互斥锁，在 Rust 中 Drop 自动调用析构函数，
//     // 但如果有自定义销毁函数则可调用相应 destroy 函数：
//     // mythread_cond_destroy(&coder.cond);
//     // mythread_mutex_destroy(&coder.mutex);
//     // coder 的内存由 Box 自动释放
// }

// // stream_encoder_mt_update: 更新编码器选项
// pub fn stream_encoder_mt_update(
//     coder_ptr: &mut LzmaStreamEncoderMt,
//     allocator: &LzmaAllocator,
//     filters: &[LzmaFilter],
//     _reversed_filters: &[LzmaFilter], // 未使用参数
// ) -> LzmaRet {
//     // 当编码 Index 或 Stream Footer 时，不允许更新选项
//     if coder_ptr.sequence > StreamSequence::SEQBLOCK {
//         return LzmaRet::ProgError;
//     }
//     // 工作线程正在编码中时也不允许更新选项
//     if coder_ptr.thr.is_some() {
//         return LzmaRet::ProgError;
//     }
//     // 检查过滤器链，如果过滤器链内存使用返回 UINT64_MAX 则表明选项无效
//     if lzma_raw_encoder_memusage(filters) == u64::MAX {
//         return LzmaRet::OptionsError;
//     }
//     // 将新的过滤器链复制到临时缓冲区
//     let mut temp: [LzmaFilter; LZMA_FILTERS_MAX + 1] = Default::default();
//     let ret = lzma_filters_copy(filters, &mut temp, allocator) ;
//     if ret != LzmaRet::Ok {
//         return ret;
//     }

//     // 释放旧链和缓存
//     lzma_filters_free(&mut coder_ptr.filters, allocator);
//     lzma_filters_free(&mut coder_ptr.filters_cache, allocator);
//     // 将临时缓冲区复制回 coder->filters
//     coder_ptr.filters.copy_from_slice(&temp);
//     LzmaRet::Ok
// }
// pub const BLOCK_SIZE_MAX: u64 = u64::MAX / LZMA_THREADS_MAX;// get_options: 根据 LzmaMt 设置获取编码器选项，填充 opt_easy、filters、block_size 及 outbuf_size_max。
// pub fn get_options(
//     options: &LzmaMt,
//     opt_easy: &mut LzmaOptionsEasy,
//     filters: &mut & [LzmaFilter],
//     block_size: &mut u64,
//     outbuf_size_max: &mut u64,
// ) -> LzmaRet {
//     // 检查 options 是否有效
//     if Some(options).is_none() {
//         return LzmaRet::ProgError;
//     }
//     if options.flags != 0 || options.threads == 0 || options.threads > LZMA_THREADS_MAX as u32 {
//         return LzmaRet::OptionsError;
//     }
//     // 如果指定了过滤器链，则直接使用，否则使用 preset
//     if let Some(filt) = options.filters {
//         *filters = filt;
//     } else {
//         if lzma_easy_preset(opt_easy, options.preset) {
//             return LzmaRet::OptionsError;
//         }
//         *filters = &opt_easy.filters;
//     }
//     // Block size 设置
//     if options.block_size > 0 {
//         if options.block_size as u64 > BLOCK_SIZE_MAX {
//             return LzmaRet::OptionsError;
//         }
//         *block_size = options.block_size as u64;
//     } else {
//         *block_size = lzma_mt_block_size(*filters);
//         if *block_size == 0 {
//             return LzmaRet::OptionsError;
//         }
//         assert!(*block_size <= BLOCK_SIZE_MAX);
//     }
//     // 计算单个输出缓冲区可能需要的最大输出大小（即单个 Block 的最大编码大小）
//     *outbuf_size_max = lzma_block_buffer_bound64(*block_size);
//     if *outbuf_size_max == 0 {
//         return LzmaRet::MemError;
//     }
//     LzmaRet::Ok
// }

// // get_progress: 获取当前编码进度
// pub fn get_progress(coder_ptr: &LzmaStreamEncoderMt, progress_in: &mut u64, progress_out: &mut u64) {
//     // 锁定 coder->mutex，防止工作线程在转移进度信息时发生竞态
//     {
//         let guard = coder_ptr.mutex.lock().unwrap();
//         *progress_in = coder_ptr.progress_in;
//         *progress_out = coder_ptr.progress_out;
//         // 遍历所有已初始化线程，累加进度
//         for thr_arc in coder_ptr.threads.as_ref().unwrap_or(&Vec::new()).iter() {
//             let thr = thr_arc.lock().unwrap();
//             *progress_in += thr.progress_in;
//             *progress_out += thr.progress_out;
//         }
//     }
//     // 无返回值
// }

// /// stream_encoder_mt_init: 初始化多线程流编码器
// pub fn stream_encoder_mt_init(
//     next: &mut LzmaNextCoder,
//     allocator: &LzmaAllocator,
//     options: &LzmaMt,
// ) -> LzmaRet {
//     // 调用 LzmaNextCoder_init() 初始化 next 结构
//     // LzmaNextCoder_init(stream_encoder_mt_init as usize, next, allocator);
//     let addr = stream_encoder_mt_init as *const () as usize;
//     if addr != next.init {
//         lzma_next_end(next, allocator);
//     }
//     next.init = addr;

//     // 获取过滤器链及其它选项
//     let mut easy: LzmaOptionsEasy = Default::default();
//     let mut filters: *const LzmaFilter = std::ptr::null();
//     let mut block_size: u64 = 0;
//     let mut outbuf_size_max: u64 = 0;
//     // 如果 get_options 返回错误则返回该错误
//     return_if_error(get_options(options, &mut easy, &mut filters, &mut block_size, &mut outbuf_size_max)).unwrap();

//     // 验证过滤器链
//     if lzma_raw_encoder_memusage(unsafe { std::slice::from_raw_parts(filters, LZMA_FILTERS_MAX + 1) }) == u64::MAX {
//         return LzmaRet::OptionsError;
//     }

//     // 验证 Check ID
//     if (options.check as u32) > LZMA_CHECK_ID_MAX {
//         return LzmaRet::ProgError;
//     }
//     if !lzma_check_is_supported(options.check) {
//         return LzmaRet:: UnsupportedCheck ;
//     }

//     // 分配并初始化基础结构
//     let coder: &mut LzmaStreamEncoderMt = if next.coder.is_none() {
//         let mut c = Box::new(LzmaStreamEncoderMt::default());

//         next.coder = Some(c);
//         next.code = Some(stream_encode_mt);
//         next.end = Some(stream_encoder_mt_end);
//         next.get_progress = Some(get_progress);
//         next.update = Some(stream_encoder_mt_update);
//         // 设置 filters 和 filters_cache 的第一个元素为 LZMA_VLI_UNKNOWN
//         next.coder.as_mut().unwrap().filters[0].id = LZMA_VLI_UNKNOWN;
//         next.coder.as_mut().unwrap().filters_cache[0].id = LZMA_VLI_UNKNOWN;
//         next.coder.as_mut().unwrap().index_encoder = LzmaNextCoder_INIT;
//         next.coder.as_mut().unwrap().index = None;
//         // 将 outq 清零
//         mem::zeroed::<LzmaOutq>();
//         next.coder.as_mut().unwrap().threads = None;
//         next.coder.as_mut().unwrap().threads_max = 0;
//         next.coder.as_mut().unwrap().threads_initialized = 0;
//         next.coder.as_mut().unwrap().thr = None;
//         // 返回新分配的 coder
//         next.coder.as_mut().unwrap()
//     } else {
//         next.coder.as_mut().unwrap()
//     };

//     // 基本初始化
//     coder.sequence = StreamSequence::SEQSTREAMHEADER;
//     coder.block_size = block_size as usize;
//     coder.outbuf_alloc_size = outbuf_size_max as usize;
//     coder.thread_error = LzmaRet::Ok;
//     coder.thr = None;

//     // 分配线程相关结构
//     assert!(options.threads > 0);
//     if coder.threads_max != options.threads {
//         // 先结束旧线程
//         threads_end(coder, allocator);
//         coder.threads = None;
//         coder.threads_max = 0;
//         coder.threads_initialized = 0;
//         coder.threads_free = None;
//         coder.threads = Some(
//             lzma_alloc(options.threads as usize * std::mem::size_of::<WorkerThread>(), allocator)
//                 as *mut Vec<WorkerThread>
//                 )
//                 .map(|ptr| unsafe { Box::from_raw(ptr) })
//                 .unwrap_or_else(|| return LzmaRet::MemError);
//         coder.threads_max = options.threads;
//     } else {
//         // 重复使用旧线程，停止正在运行的线程
//         threads_stop(coder, true);
//     }

//     // 初始化输出队列
//     return_if_error(lzma_outq_init(&mut coder.outq, allocator, options.threads)).unwrap();

//     // 超时设置
//     coder.timeout = options.timeout;

//     // 释放旧 Filter 链及缓存
//     lzma_filters_free(&mut coder.filters, allocator);
//     lzma_filters_free(&mut coder.filters_cache, allocator);
//     // 复制新的 Filter 链到 coder->filters
//     return_if_error(lzma_filters_copy(unsafe {
//         std::slice::from_raw_parts(filters, LZMA_FILTERS_MAX + 1)
//     }, &mut coder.filters, allocator)).unwrap();

//     // Index
//     lzma_index_end(coder.index.take(), allocator);
//     coder.index = lzma_index_init(allocator);
//     if coder.index.is_none() {
//         return LzmaRet::MemError;
//     }

//     // Stream Header 设置
//     coder.stream_flags.version = 0;
//     coder.stream_flags.check = options.check;
//     return_if_error(lzma_stream_header_encode(&mut coder.stream_flags, &mut coder.header))
//         .unwrap();
//     coder.header_pos = 0;

//     // 进度信息
//     coder.progress_in = 0;
//     // 假定 LZMA_STREAM_HEADER_SIZE 为常量
//     coder.progress_out = LZMA_STREAM_HEADER_SIZE as u64;

//     LzmaRet::Ok
// }

// /// lzma_stream_encoder_mt: 初始化多线程流编码器并设置支持的动作
// pub fn lzma_stream_encoder_mt(strm: &mut lzma_stream, options: &LzmaMt) -> LzmaRet {
//     // let ret = lzma_next_strm_init(stream_encoder_mt_init, strm, options);
//     // if ret != LzmaRet::Ok {
//     //     return ret;
//     // }

//     let ret: LzmaRet = lzma_strm_init(Some(strm));
//     if ret != LzmaRet::Ok {
//         return ret;
//     }
//     let ret: LzmaRet = stream_encoder_mt_init(
//         &mut strm.internal.as_mut().unwrap().next.as_mut().unwrap(),
//         strm.allocator,
//         options,
//     );
//     if ret != LzmaRet::Ok {
//         lzma_end(Some(strm));
//         return ret;
//     }

//     {
//         let internal = strm.internal.as_mut().unwrap();
//         internal.supported_actions[LzmaAction::Run as usize] = true;
//         internal.supported_actions[LzmaAction::FullFlush as usize] = true;
//         internal.supported_actions[LzmaAction::FullBarrier as usize] = true;
//         internal.supported_actions[LzmaAction::Finish as usize] = true;
//     }
//     LzmaRet::Ok
// }

// /// lzma_stream_encoder_mt_memusage: 计算多线程流编码器的内存使用量
// pub fn lzma_stream_encoder_mt_memusage(options: &LzmaMt) -> u64 {
//     let mut easy: LzmaOptionsEasy = Default::default();
//     let mut filters: *const LzmaFilter = std::ptr::null();
//     let mut block_size: u64 = 0;
//     let mut outbuf_size_max: u64 = 0;
//     if get_options(options, &mut easy, &mut filters, &mut block_size, &mut outbuf_size_max) != LzmaRet::Ok {
//         return u64::MAX;
//     }
//     // 内部输入缓冲区的内存使用量
//     let inbuf_memusage = (options.threads as u64) * block_size;
//     let mut filters_memusage = lzma_raw_encoder_memusage(unsafe {
//         std::slice::from_raw_parts(filters, LZMA_FILTERS_MAX + 1)
//     });
//     if filters_memusage == u64::MAX {
//         return u64::MAX;
//     }
//     filters_memusage *= options.threads as u64;
//     let outq_memusage = lzma_outq_memusage(outbuf_size_max, options.threads);
//     if outq_memusage == u64::MAX {
//         return u64::MAX;
//     }
//     // 总内存 = 基础内存 + LzmaStreamEncoderMt + 线程结构体
//     let mut total_memusage = LZMA_MEMUSAGE_BASE
//         + std::mem::size_of::<LzmaStreamEncoderMt>() as u64
//         + (options.threads as u64) * std::mem::size_of::<WorkerThread>() as u64;
//     if u64::MAX - total_memusage < inbuf_memusage {
//         return u64::MAX;
//     }
//     total_memusage += inbuf_memusage;
//     if u64::MAX - total_memusage < filters_memusage {
//         return u64::MAX;
//     }
//     total_memusage += filters_memusage;
//     if u64::MAX - total_memusage < outq_memusage {
//         return u64::MAX;
//     }
//     total_memusage + outq_memusage
// }
