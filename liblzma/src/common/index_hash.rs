use crate::{api::LzmaVli, check::LzmaCheckState};

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
