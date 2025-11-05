use crate::api::{LzmaOptionsType, LzmaVli};

use super::{lzma_next_filter_init, LzmaInitFunction};

pub type MemUsageFunction = fn(options: &LzmaOptionsType) -> u64;

/// 过滤器编码器和解码器的公共结构体
#[derive(Debug, Clone)]
pub struct LzmaFilterCoder {
    /// 过滤器 ID
    pub id: LzmaVli, // 假设 lzma_vli 是 u64 类型

    /// 初始化过滤器编码器并调用 lzma_next_filter_init() 用于 filters + 1
    pub init: Option<LzmaInitFunction>,

    /// 计算编码器的内存使用量。如果选项无效，则返回 u64::MAX
    pub memusage: Option<MemUsageFunction>,
}

// 实现 LzmaFilterCoder 的 Default 特性
impl Default for LzmaFilterCoder {
    fn default() -> Self {
        Self {
            // 默认过滤器 ID 设为 0
            id: 0,
            // 默认初始化函数，调用 lzma_next_filter_init 瞎给的函数，编译过了再说吧
            init: Some(lzma_next_filter_init),
            // 默认内存使用量计算函数为空
            memusage: None,
        }
    }
}

/// 查找过滤器的函数类型
pub type LzmaFilterFind = fn(id: LzmaVli) -> Option<LzmaFilterCoder>;

#[derive(Debug, Clone)]
struct FilterFeatures {
    /// 过滤器 ID
    id: LzmaVli, // 使用 LzmaVli 类型

    options: LzmaOptionsType,
    /// 过滤器特定选项结构体的大小
    options_size: usize,

    /// 如果此过滤器可以用作链中的非最后过滤器，则为 true
    non_last_ok: bool,

    /// 如果此过滤器可以用作链中的最后过滤器，则为 true
    last_ok: bool,

    /// 如果过滤器可能改变数据大小（即编码输出的数量可能与未压缩输入的数量不同），则为 true
    changes_size: bool,
}
