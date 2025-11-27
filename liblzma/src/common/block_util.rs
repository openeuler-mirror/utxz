use crate::{
    api::{
        LzmaBlock, LzmaCheck, LzmaRet, LZMA_BLOCK_HEADER_SIZE_MAX, LZMA_BLOCK_HEADER_SIZE_MIN,
        LZMA_CHECK_ID_MAX, LZMA_VLI_UNKNOWN,
    },
    check::lzma_check_size,
};

use super::index::{vli_ceil4, UNPADDED_SIZE_MAX, UNPADDED_SIZE_MIN};

pub fn lzma_block_compressed_size(block: &mut LzmaBlock, unpadded_size: u64) -> LzmaRet {
    // Validate everything but Uncompressed Size and filters.
    if lzma_block_unpadded_size(block) == 0 {
        return LzmaRet::ProgError;
    }

    let container_size = block.header_size as u64 + lzma_check_size(block.check.clone()) as u64;

    // Validate that Compressed Size will be greater than zero.
    if unpadded_size <= container_size {
        return LzmaRet::DataError;
    }

    // Calculate what Compressed Size is supposed to be.
    // If Compressed Size was present in Block Header,
    // compare that the new value matches it.
    let compressed_size = unpadded_size - container_size;
    if block.compressed_size != LZMA_VLI_UNKNOWN && block.compressed_size != compressed_size {
        return LzmaRet::DataError;
    }

    block.compressed_size = compressed_size;

    LzmaRet::Ok
}

pub fn lzma_block_unpadded_size(block: &LzmaBlock) -> u64 {
    // Validate the values that we are interested in i.e. all but
    // Uncompressed Size and the filters.
    if block.version > 1
        || block.header_size < LZMA_BLOCK_HEADER_SIZE_MIN
        || block.header_size > LZMA_BLOCK_HEADER_SIZE_MAX
        || (block.header_size & 3) != 0
        || block.compressed_size == 0
        || block.check.clone() as u32 > LZMA_CHECK_ID_MAX
    {
        return 0;
    }

    // If Compressed Size is unknown, return that we cannot know size of the Block either.
    if block.compressed_size == LZMA_VLI_UNKNOWN {
        return LZMA_VLI_UNKNOWN;
    }

    // Calculate Unpadded Size and validate it.
    let unpadded_size = block.compressed_size
        + block.header_size as u64
        + lzma_check_size(block.check.clone()) as u64;

    assert!(unpadded_size >= UNPADDED_SIZE_MIN);

    if unpadded_size > UNPADDED_SIZE_MAX {
        return 0;
    }

    unpadded_size
}

pub fn lzma_block_total_size(block: &LzmaBlock) -> u64 {
    let mut unpadded_size = lzma_block_unpadded_size(block);

    if unpadded_size != LZMA_VLI_UNKNOWN {
        unpadded_size = vli_ceil4(unpadded_size);
    }

    unpadded_size
}
