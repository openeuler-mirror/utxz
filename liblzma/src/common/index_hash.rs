/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::{
    api::{LzmaCheck, LzmaRet, LzmaVli, LZMA_BACKWARD_SIZE_MAX, LZMA_VLI_MAX},
    check::{
        lzma_check_finish, lzma_check_init, lzma_check_size, lzma_check_update, lzma_crc32,
        LzmaCheckState,
    },
};

use super::{
    index_size, index_size_unpadded, index_stream_size, lzma_vli_decode, lzma_vli_size, vli_ceil4,
    INDEX_INDICATOR, UNPADDED_SIZE_MAX, UNPADDED_SIZE_MIN,
};

/// LZMA 索引哈希信息结构体
#[derive(Debug, Clone, Default)]
pub struct LzmaIndexHashInfo {
    /// 块大小的总和（包括块填充）
    blocks_size: LzmaVli,

    /// 未压缩大小字段的总和
    uncompressed_size: LzmaVli,

    /// 记录的数量
    count: LzmaVli,

    /// 索引记录列表的大小（以字节为单位）
    index_list_size: LzmaVli,

    /// 从未填充大小和未压缩大小计算的校验
    check: LzmaCheckState,
}

/// LZMA 索引哈希结构体
#[derive(Debug, Clone, Default)]
pub struct LzmaIndexHash {
    /// 解码过程中的当前序列状态
    pub sequence: Sequence,

    /// 解码实际块时收集的信息
    pub blocks: LzmaIndexHashInfo,

    /// 从索引字段收集的信息
    pub records: LzmaIndexHashInfo,

    /// 尚未完全解码的记录数量
    pub remaining: LzmaVli,

    /// 当前从索引记录读取的未填充大小
    pub unpadded_size: LzmaVli,

    /// 当前从索引记录读取的未压缩大小
    pub uncompressed_size: LzmaVli,

    /// 解码可变长度整数时在记录列表中的位置
    pub pos: usize,

    /// 索引的 CRC32 校验值
    pub crc32: u32,
}

/// 解码序列的枚举，表示解码过程中的不同阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sequence {
    #[default]
    SeqBlock,
    SeqCount,
    SeqUnpadded,
    SeqUncompressed,
    SeqPaddingInit,
    SeqPadding,
    SeqCrc32,
}

/// 初始化 LZMA 索引哈希结构体
pub fn lzma_index_hash_init(index_hash: Option<Box<LzmaIndexHash>>) -> Box<LzmaIndexHash> {
    let mut index_hash = match index_hash {
        Some(hash) => hash,
        None => {
            // 使用 Box 分配到堆上，并借用其内部数据
            let boxed_hash = Box::new(LzmaIndexHash::default());
            boxed_hash
        }
    };

    // 初始化默认值
    index_hash.sequence = Sequence::SeqBlock;
    index_hash.blocks.blocks_size = 0;
    index_hash.blocks.uncompressed_size = 0;
    index_hash.blocks.count = 0;
    index_hash.blocks.index_list_size = 0;
    index_hash.records.blocks_size = 0;
    index_hash.records.uncompressed_size = 0;
    index_hash.records.count = 0;
    index_hash.records.index_list_size = 0;
    index_hash.unpadded_size = 0;
    index_hash.uncompressed_size = 0;
    index_hash.pos = 0;
    index_hash.crc32 = 0;

    // 假设 lzma_check_init 是一个安全的函数，可以初始化 check
    lzma_check_init(&mut index_hash.blocks.check, LzmaCheck::Crc32);
    lzma_check_init(&mut index_hash.records.check, LzmaCheck::Crc32);

    index_hash
}

/// 结束并释放 LZMA 索引哈希结构体
pub fn lzma_index_hash_end(index_hash: &mut LzmaIndexHash) {}

/// 获取 LZMA 索引哈希的大小
pub fn lzma_index_hash_size(index_hash: &LzmaIndexHash) -> LzmaVli {
    // 从 blocks 而不是 records 获取索引的大小，以便在解码索引之前
    // 应用程序可以知道索引大小
    index_size(index_hash.blocks.count, index_hash.blocks.index_list_size)
}

/// 向 LZMA 索引哈希信息中追加数据
fn hash_append(info: &mut LzmaIndexHashInfo, unpadded_size: LzmaVli, uncompressed_size: LzmaVli) {
    info.blocks_size += vli_ceil4(unpadded_size);
    info.uncompressed_size += uncompressed_size;
    info.index_list_size +=
        (lzma_vli_size(unpadded_size) + lzma_vli_size(uncompressed_size)) as u64;
    info.count += 1;

    // 创建包含 unpadded_size 和 uncompressed_size 的数组
    let sizes: [LzmaVli; 2] = [unpadded_size, uncompressed_size];

    let mut bytes: Vec<u8> = Vec::with_capacity(sizes.len() * std::mem::size_of::<LzmaVli>());
    for size in sizes.iter() {
        let size_bytes = size.to_le_bytes(); // 转换为小端字节序
        bytes.extend_from_slice(&size_bytes);
    }
    // 更新校验状态
    lzma_check_update(
        &mut info.check,
        LzmaCheck::Crc32,
        &bytes,
        bytes.len() as usize,
    );

    // 该函数在 Rust 中没有返回值，因此不需要显式的 return
}

/// 向 LZMA 索引哈希中追加数据，并进行参数验证和更新
pub fn lzma_index_hash_append(
    index_hash: &mut LzmaIndexHash,
    unpadded_size: LzmaVli,
    uncompressed_size: LzmaVli,
) -> LzmaRet {
    // 验证参数
    if Some(index_hash.clone()).is_none()
        || index_hash.sequence != Sequence::SeqBlock
        || unpadded_size < UNPADDED_SIZE_MIN
        || unpadded_size > UNPADDED_SIZE_MAX
        || uncompressed_size > LZMA_VLI_MAX
    {
        return LzmaRet::ProgError;
    }

    // 更新哈希
    hash_append(&mut index_hash.blocks, unpadded_size, uncompressed_size);

    // 验证 info 的属性仍然在允许的限制内
    if index_hash.blocks.blocks_size > LZMA_VLI_MAX
        || index_hash.blocks.uncompressed_size > LZMA_VLI_MAX
        || index_size(index_hash.blocks.count, index_hash.blocks.index_list_size)
            > LZMA_BACKWARD_SIZE_MAX
        || index_stream_size(
            index_hash.blocks.blocks_size,
            index_hash.blocks.count,
            index_hash.blocks.index_list_size,
        ) > LZMA_VLI_MAX
    {
        return LzmaRet::DataError;
    }

    LzmaRet::Ok
}

fn check_buffers_equal(buf1: &[u8], buf2: &[u8], size: usize) -> bool {
    if buf1.len() < size || buf2.len() < size {
        return false;
    }
    &buf1[..size] == &buf2[..size]
}
pub fn lzma_index_hash_decode(
    index_hash: &mut LzmaIndexHash,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    // 检查输入缓冲区
    if *in_pos >= in_size {
        return LzmaRet::BufError;
    }

    let in_start = *in_pos;
    let mut ret = LzmaRet::Ok;

    while *in_pos < in_size {
        match index_hash.sequence {
            Sequence::SeqBlock => {
                // 检查索引指示符是否存在
                if input[*in_pos] != INDEX_INDICATOR {
                    return LzmaRet::DataError;
                }
                *in_pos += 1;
                index_hash.sequence = Sequence::SeqCount;
            }

            Sequence::SeqCount => {
                ret = lzma_vli_decode(
                    &mut index_hash.remaining,
                    Some(&mut index_hash.pos),
                    input,
                    in_pos,
                    in_size,
                );
                if ret != LzmaRet::StreamEnd {
                    break;
                }

                // 计数必须与已解码的块数匹配
                if index_hash.remaining != index_hash.blocks.count {
                    return LzmaRet::DataError;
                }

                ret = LzmaRet::Ok;
                index_hash.pos = 0;

                // 处理没有块的特殊情况
                index_hash.sequence = if index_hash.remaining == 0 {
                    Sequence::SeqPaddingInit
                } else {
                    Sequence::SeqUnpadded
                };
            }

            Sequence::SeqUnpadded | Sequence::SeqUncompressed => {
                let size = if index_hash.sequence == Sequence::SeqUnpadded {
                    &mut index_hash.unpadded_size
                } else {
                    &mut index_hash.uncompressed_size
                };

                ret = lzma_vli_decode(size, Some(&mut index_hash.pos), input, in_pos, in_size);
                if ret != LzmaRet::StreamEnd {
                    break;
                }

                ret = LzmaRet::Ok;
                index_hash.pos = 0;

                if index_hash.sequence == Sequence::SeqUnpadded {
                    if index_hash.unpadded_size < UNPADDED_SIZE_MIN
                        || index_hash.unpadded_size > UNPADDED_SIZE_MAX
                    {
                        return LzmaRet::DataError;
                    }
                    index_hash.sequence = Sequence::SeqUncompressed;
                } else {
                    // 更新哈希
                    hash_append(
                        &mut index_hash.records,
                        index_hash.unpadded_size,
                        index_hash.uncompressed_size,
                    );

                    // 验证不超过已知大小
                    if index_hash.blocks.blocks_size < index_hash.records.blocks_size
                        || index_hash.blocks.uncompressed_size
                            < index_hash.records.uncompressed_size
                        || index_hash.blocks.index_list_size < index_hash.records.index_list_size
                    {
                        return LzmaRet::DataError;
                    }

                    // 检查是否是最后一个记录
                    index_hash.remaining -= 1;
                    index_hash.sequence = if index_hash.remaining == 0 {
                        Sequence::SeqPaddingInit
                    } else {
                        Sequence::SeqUnpadded
                    };
                }
            }

            Sequence::SeqPaddingInit => {
                let padding_size = index_size_unpadded(
                    index_hash.records.count,
                    index_hash.records.index_list_size,
                );
                // 计算需要多少填充字节来使索引大小成为4的倍数
                // 使用安全的模运算，避免整数下溢出
                let remainder = padding_size % 4;
                index_hash.pos = if remainder == 0 {
                    0
                } else {
                    (4 - remainder) as usize
                };
                index_hash.sequence = Sequence::SeqPadding;
                continue;
            }

            Sequence::SeqPadding => {
                if index_hash.pos > 0 {
                    index_hash.pos -= 1;
                    if input[*in_pos] != 0x00 {
                        return LzmaRet::DataError;
                    }
                    *in_pos += 1;
                } else {
                    // 比较大小
                    if index_hash.blocks.blocks_size != index_hash.records.blocks_size
                        || index_hash.blocks.uncompressed_size
                            != index_hash.records.uncompressed_size
                        || index_hash.blocks.index_list_size != index_hash.records.index_list_size
                    {
                        return LzmaRet::DataError;
                    }

                    // 完成哈希并比较
                    lzma_check_finish(&mut index_hash.blocks.check, LzmaCheck::Crc32);
                    lzma_check_finish(&mut index_hash.records.check, LzmaCheck::Crc32);

                    unsafe {
                        if !check_buffers_equal(
                            &index_hash.blocks.check.buffer.u8,
                            &index_hash.records.check.buffer.u8,
                            lzma_check_size(LzmaCheck::Crc32) as usize,
                        ) {
                            return LzmaRet::DataError;
                        }
                    }

                    // 完成 CRC32 计算
                    index_hash.crc32 = lzma_crc32(
                        &input[in_start..*in_pos],
                        *in_pos - in_start as usize,
                        index_hash.crc32,
                    );
                    index_hash.sequence = Sequence::SeqCrc32;
                    continue;
                }
            }

            Sequence::SeqCrc32 => {
                while index_hash.pos < 4 {
                    if *in_pos == in_size {
                        return LzmaRet::Ok;
                    }

                    let expected = (index_hash.crc32 >> (index_hash.pos * 8)) & 0xFF;
                    if expected as u8 != input[*in_pos] {
                        return LzmaRet::DataError;
                    }
                    *in_pos += 1;
                    index_hash.pos += 1;
                }
                return LzmaRet::StreamEnd;
            }
        }
    }

    // 更新 CRC32
    let in_used = *in_pos - in_start;
    if in_used > 0 {
        index_hash.crc32 = lzma_crc32(
            &input[in_start..*in_pos],
            in_used as usize,
            index_hash.crc32,
        );
    }

    ret
}
