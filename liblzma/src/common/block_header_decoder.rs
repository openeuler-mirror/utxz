/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use common::read32le;

use crate::{
    api::{LzmaBlock, LzmaRet, LZMA_CHECK_ID_MAX, LZMA_VLI_UNKNOWN},
    check::lzma_crc32,
    lzma_block_header_size_decode,
};

use super::{
    lzma_block_unpadded_size, lzma_filter_flags_decode, lzma_filters_free, lzma_vli_decode,
};

pub fn lzma_block_header_decode(block: &mut LzmaBlock, input: &mut [u8]) -> LzmaRet {
    // 注意：我们认为以下情况头部数据损坏：
    // - CRC32 不匹配
    // - 可变长度整数无效或超过63位
    // - 头部太小，无法包含声明的信息

    // 检查 block 的 filters 是否已初始化
    if block.filters.is_empty() {
        return LzmaRet::ProgError;
    }

    // 初始化过滤器选项数组
    // 这样即使函数出错，调用者也可以安全地释放选项
    for i in 0..5 {
        block.filters[i as usize].id = LZMA_VLI_UNKNOWN;
        block.filters[i].options = None;
    }

    // 支持版本0和1。如果指定了更新的版本，需要降级
    if block.version > 1 {
        block.version = 1;
    }

    // 这不是块头选项，但由于解压缩器会在 version >= 1 时读取它
    // 在这里初始化比期望调用者来做更好，因为在几乎所有情况下都应该是 false
    block.ignore_check = false;

    // 验证块头大小和校验类型
    // 调用者必须已经设置这些，所以如果此测试失败就是编程错误
    if lzma_block_header_size_decode!(input[0]) != block.header_size
        || (block.check.clone() as u32) > LZMA_CHECK_ID_MAX
    {
        return LzmaRet::ProgError;
    }

    // 排除 CRC32 字段
    let in_size = block.header_size - 4;

    // 验证 CRC32
    let crc = lzma_crc32(&input[0..in_size as usize], in_size as usize, 0);
    let crc_input = read32le(&input[in_size as usize..]);
    if crc != crc_input {
        println!("crc: {:?}", crc);
        println!("crc_input: {:?}", crc_input);
        println!("input: {:?}", &input[..in_size as usize]);
        return LzmaRet::DataError;
    }

    // 检查不支持的标志
    if (input[1] & 0x3C) != 0 {
        return LzmaRet::OptionsError;
    }

    // 从块头大小和块标志字段之后开始
    let mut in_pos = 2;

    // 压缩大小
    if (input[1] & 0x40) != 0 {
        let ret = lzma_vli_decode(
            &mut block.compressed_size,
            Some(&mut 0),
            input,
            &mut in_pos,
            in_size as usize,
        );
        if ret != LzmaRet::Ok {
            return ret;
        }

        // 验证压缩大小
        // 这检查它不是零并且块的总大小是有效的 VLI
        if lzma_block_unpadded_size(block) == 0 {
            return LzmaRet::DataError;
        }
    } else {
        block.compressed_size = LZMA_VLI_UNKNOWN;
    }

    // 未压缩大小
    if (input[1] & 0x80) != 0 {
        let ret = lzma_vli_decode(
            &mut block.uncompressed_size,
            Some(&mut 0),
            input,
            &mut in_pos,
            in_size as usize,
        );
        if ret != LzmaRet::Ok {
            return ret;
        }
    } else {
        block.uncompressed_size = LZMA_VLI_UNKNOWN;
    }

    // 过滤器标志
    let filter_count = (input[1] & 3) + 1;
    for i in 0..filter_count {
        let ret = lzma_filter_flags_decode(
            &mut block.filters[i as usize],
            input,
            &mut in_pos,
            in_size.try_into().unwrap(),
        );
        if ret != LzmaRet::Ok {
            lzma_filters_free(&mut block.filters);
            return ret;
        }
    }

    // 填充
    while in_pos < in_size as usize {
        if input[in_pos as usize] != 0x00 {
            lzma_filters_free(&mut block.filters);
            // 可能存在一些新字段，所以使用 LZMA_OPTIONS_ERROR
            // 而不是 LZMA_DATA_ERROR
            return LzmaRet::OptionsError;
        }
        in_pos += 1;
    }

    LzmaRet::Ok
}
