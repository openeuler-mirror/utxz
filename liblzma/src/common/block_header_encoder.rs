/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::write32le;

use crate::{
    api::{lzma_vli_is_valid, LzmaBlock, LzmaRet, LZMA_FILTERS_MAX, LZMA_VLI_UNKNOWN},
    check::lzma_crc32,
};

use super::{
    lzma_block_unpadded_size, lzma_filter_flags_encode, lzma_filter_flags_size, lzma_vli_encode,
    lzma_vli_size,
};

pub fn lzma_block_header_size(block: &mut LzmaBlock) -> LzmaRet {
    if block.version > 1 {
        return LzmaRet::OptionsError;
    }

    // Block Header Size + Block Flags + CRC32
    let mut size = 1 + 1 + 4;

    // Compressed Size
    if block.compressed_size != LZMA_VLI_UNKNOWN {
        let add = lzma_vli_size(block.compressed_size);
        if add == 0 || block.compressed_size == 0 {
            return LzmaRet::ProgError;
        }
        size += add;
    }

    // Uncompressed Size
    if block.uncompressed_size != LZMA_VLI_UNKNOWN {
        let add = lzma_vli_size(block.uncompressed_size);
        if add == 0 {
            return LzmaRet::ProgError;
        }
        size += add;
    }

    // List of Filter Flags
    if block.filters.is_empty() || block.filters[0].id == LZMA_VLI_UNKNOWN {
        return LzmaRet::ProgError;
    }

    let mut i: usize = 0;
    while block.filters[i].id != LZMA_VLI_UNKNOWN {
        if i == LZMA_FILTERS_MAX {
            return LzmaRet::ProgError;
        }
        let mut add: u32 = 0;
        let ret = lzma_filter_flags_size(&mut add, &block.filters[i]);
        if ret != LzmaRet::Ok {
            return ret;
        }

        size += add;
        i += 1;
    }

    // Pad to a multiple of four bytes
    block.header_size = (size + 3) & !3;

    LzmaRet::Ok
}

pub fn lzma_block_header_encode(block: &LzmaBlock, output: &mut [u8]) -> LzmaRet {
    if lzma_block_unpadded_size(block) == 0 || !lzma_vli_is_valid(block.uncompressed_size) {
        return LzmaRet::ProgError;
    }

    let out_size = block.header_size - 4;
    output[0] = (out_size / 4) as u8;

    output[1] = 0x00;
    let mut out_pos = 2;

    // Compressed Size
    if block.compressed_size != LZMA_VLI_UNKNOWN {
        let ret = lzma_vli_encode(
            block.compressed_size,
            None,
            output,
            &mut out_pos,
            out_size as usize,
        );
        if ret != LzmaRet::Ok {
            return LzmaRet::ProgError;
        }

        output[1] |= 0x40;
    }

    // Uncompressed Size
    if block.uncompressed_size != LZMA_VLI_UNKNOWN {
        let ret = lzma_vli_encode(
            block.uncompressed_size,
            None,
            output,
            &mut out_pos,
            out_size as usize,
        );
        if ret != LzmaRet::Ok {
            return LzmaRet::ProgError;
        }
        output[1] |= 0x80;
    }

    // Filter Flags
    // println!("block.filters[0].id = {}", block.filters[0].id);
    if block.filters.is_empty() || block.filters[0].id == LZMA_VLI_UNKNOWN {
        return LzmaRet::ProgError;
    }

    let mut filter_count = 0;
    // for filter in block.filters.iter() {
    while block.filters[filter_count].id != LZMA_VLI_UNKNOWN {
        if filter_count == LZMA_FILTERS_MAX {
            return LzmaRet::ProgError;
        }

        let ret = lzma_filter_flags_encode(
            &block.filters[filter_count],
            output,
            &mut out_pos,
            out_size as usize,
        );
        if ret != LzmaRet::Ok {
            return ret;
        }
        filter_count += 1;
    }

    output[1] |= filter_count as u8 - 1;

    // Padding
    output[out_pos..out_size as usize].fill(0);

    // CRC32
    let crc = lzma_crc32(&output[..out_size as usize], out_size as usize, 0);
    write32le(&mut output[out_size as usize..], crc);

    LzmaRet::Ok
}
