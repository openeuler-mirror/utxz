#![deny(clippy::absurd_extreme_comparisons)]
#![deny(clippy::useless_attribute)]
use crate::api::{LzmaOptionsType, LzmaRet};

use super::LzmaInitFunction;
use crate::common::filter_common::MemUsageFunction;
/// 过滤器编码器结构体
#[derive(Debug, Clone, Default)]
pub struct LzmaFilterEncoder {
    /// 过滤器 ID
    pub id: u64, // lzma_vli 在 Rust 中通常对应 u64 类型

    /// 初始化过滤器编码器，并调用 lzma_next_filter_init()，参数为过滤器数量 + 1。
    pub init: Option<LzmaInitFunction>,

    /// 计算编码器的内存使用量。如果选项无效，返回 u64::MAX。
    pub memusage: Option<MemUsageFunction>,

    /// 计算推荐的未压缩大小，以便将输入数据拆分为多个 .xz 块，使多线程编码成为可能。
    /// 如果为 None，则假定编码器在单线程下足够快。
    pub block_size: Option<BlockSizeFunction>,

    /// 告知过滤器属性字段的大小。如果选项无效，返回 LzmaRet::OptionsError，并将 size 设置为 u32::MAX。
    pub props_size_get: Option<PropsSizeGetFunction>,

    /// 某些过滤器的属性字段大小始终相同。如果 props_size_get 为 None，则使用此值。
    pub props_size_fixed: u32,

    /// 编码过滤器属性。
    ///
    /// # 返回值
    /// - `LzmaRet::Ok`: 属性编码成功。
    /// - `LzmaRet::OptionsError`: 不支持的选项。
    /// - `LzmaRet::ProgError`: 无效选项或输出空间不足。
    pub props_encode: Option<PropsEncodeFunction>,
}

pub type BlockSizeFunction = fn(options: &LzmaOptionsType) -> u64;
pub type PropsSizeGetFunction = fn(size: &mut u32, options: &LzmaOptionsType) -> LzmaRet;
pub type PropsEncodeFunction = fn(options: &LzmaOptionsType, out: &mut [u8]) -> LzmaRet;

impl LzmaFilterEncoder {
    /// 创建一个新的 LzmaFilterEncoder 实例，使用默认值
    pub fn new(id: u64) -> Self {
        Self {
            id,
            init: None,
            memusage: None,
            block_size: None,
            props_size_get: None,
            props_size_fixed: 0,
            props_encode: None,
        }
    }
}

// impl Default for LzmaFilterEncoder {
//     fn default() -> Self {
//         Self {
//             id: 0,
//             init: None,
//             memusage: None,
//             block_size: None,
//             props_size_get: None,
//             props_size_fixed: 0,
//             props_encode: None,
//         }
//     }
// }
