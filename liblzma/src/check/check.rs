/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::check::crc32_small::*;
use crate::check::crc64_small::*;
use std::fmt;

use crate::{
    api::{LzmaBool, LzmaCheck, LZMA_CHECK_ID_MAX},
    check::{lzma_crc32, lzma_crc64, lzma_sha256_finish, lzma_sha256_init, lzma_sha256_update},
};

// /// LZMA 校验状态结构体
// pub struct LzmaCheckState {
//     /// 用于保存最终结果的缓冲区和用于 SHA256 的临时缓冲区
//     buffer: Buffer,

//     /// 校验特定的数据
//     state: CheckState,
// }

// /// 缓冲区联合体
// pub enum Buffer {
//     U8([u8; 64]),
//     U32([u32; 16]),
//     U64([u64; 8]),
// }

// /// 校验状态联合体
// pub enum CheckState {
//     Crc32(u32),
//     Crc64(u64),
//     Sha256(LzmaSha256State),
// }

// /// LZMA SHA256 状态结构体
// pub struct LzmaSha256State {
//     /// 内部状态，包含 8 个 32 位无符号整数
//     state: [u32; 8],

//     /// 消息的大小，不包括填充
//     size: u64,
// }

pub fn lzma_check_is_supported(type_: LzmaCheck) -> bool {
    if type_.clone() as u32 > LZMA_CHECK_ID_MAX {
        return false;
    }

    const AVAILABLE_CHECKS: [bool; LZMA_CHECK_ID_MAX as usize + 1] = [
        true, // LZMA_CHECK_NONE
        true, false, // Reserved
        false, // Reserved
        true, false, // Reserved
        false, // Reserved
        false, // Reserved
        false, // Reserved
        false, // Reserved
        true, false, // Reserved
        false, // Reserved
        false, // Reserved
        false, // Reserved
        false, // Reserved
    ];

    AVAILABLE_CHECKS[type_ as usize]
}

pub fn lzma_check_size(type_: LzmaCheck) -> u32 {
    if type_.clone() as u32 > LZMA_CHECK_ID_MAX {
        return u32::MAX;
    }

    const CHECK_SIZES: [u8; LZMA_CHECK_ID_MAX as usize + 1] =
        [0, 4, 4, 4, 8, 8, 8, 16, 16, 16, 32, 32, 32, 64, 64, 64];

    CHECK_SIZES[type_ as usize] as u32
}

#[derive(Debug, Copy, Clone, Default)]
pub struct LzmaSha256State {
    pub state: [u32; 8],
    pub size: u64,
}

// impl Default for LzmaSha256State {
//     fn default() -> Self {
//         LzmaSha256State {
//             state: [
//                 0x6A09E667,
//                 0xBB67AE85,
//                 0x3C6EF372,
//                 0xA54FF53A,
//                 0x510E527F,
//                 0x9B05688C,
//                 0x1F83D9AB,
//                 0x5BE0CD19,
//             ],
//             size: 0,
//         }
//     }
// }
#[derive(Default, Debug, Clone)]
pub struct LzmaCheckState {
    pub buffer: Buffer,
    pub state: State,
}

#[derive(Clone, Debug)]
pub struct Buffer {
    /// 按字节视图，固定长度 64 字节
    pub u8: [u8; 64],
    /// 按 32 位整型视图，固定长度 16 个 u32
    pub u32: [u32; 16],
    /// 按 64 位整型视图，固定长度 8 个 u64
    pub u64: [u64; 8],
}

impl Default for Buffer {
    fn default() -> Self {
        Buffer {
            u8: [0u8; 64],
            u32: [0u32; 16],
            u64: [0u64; 8],
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct State {
    /// CRC32 校验值
    pub crc32: u32,
    /// CRC64 校验值
    pub crc64: u64,
    /// SHA256 校验状态
    pub sha256: LzmaSha256State,
}

pub fn lzma_check_init(check: &mut LzmaCheckState, type_: LzmaCheck) {
    match type_ {
        LzmaCheck::None => { /* 不做任何操作 */ }
        LzmaCheck::Crc32 => {
            check.state.crc32 = 0;
        }
        LzmaCheck::Crc64 => {
            check.state.crc64 = 0;
        }
        LzmaCheck::Sha256 => {
            lzma_sha256_init(check);
        }
        _ => {}
    }
}

pub fn lzma_check_update(check: &mut LzmaCheckState, type_: LzmaCheck, buf: &[u8], size: usize) {
    match type_ {
        LzmaCheck::Crc32 => {
            check.state.crc32 = lzma_crc32(buf, size, unsafe { check.state.crc32 });
        }
        LzmaCheck::Crc64 => {
            check.state.crc64 = lzma_crc64(buf, size, check.state.crc64);
        }
        LzmaCheck::Sha256 => {
            lzma_sha256_update(buf, size, check);
        }
        _ => {}
    }
}

pub fn lzma_check_finish(check: &mut LzmaCheckState, type_: LzmaCheck) {
    match type_ {
        LzmaCheck::Crc32 => {
            check.buffer.u32[0] = check.state.crc32;
        }

        LzmaCheck::Crc64 => {
            check.buffer.u64[0] = check.state.crc64;
        }

        LzmaCheck::Sha256 => {
            lzma_sha256_finish(check);
        }
        _ => {}
    }
}
