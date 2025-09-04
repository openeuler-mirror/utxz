use crate::{
    api::LzmaRet,
    common::{LzmaFilterInfo, LzmaNextCoder},
};

use super::SimpleType;

type FilterFn = fn(&mut SimpleType, u32, bool, &mut [u8], usize) -> usize;
/// 初始化简单编码器
pub fn lzma_simple_coder_init(
    _next: &mut LzmaNextCoder,
    _filters: &[LzmaFilterInfo],
    _filter: FilterFn,
    _simple_size: usize,
    _unfiltered_max: usize,
    _alignment: u32,
    _is_encoder: bool,
) -> LzmaRet {
    LzmaRet::Ok
}
