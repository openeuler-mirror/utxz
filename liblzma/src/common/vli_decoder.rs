/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
use crate::api::{LzmaRet, LzmaVli, LZMA_VLI_BYTES_MAX};

/// 解码可变长度整数
///
/// # 参数
/// * `vli` - 解码后的整数值
/// * `vli_pos` - 当前解码位置，单次调用模式下为 `None`
/// * `input` - 输入缓冲区
/// * `in_pos` - 输入缓冲区的当前位置
/// * `in_size` - 输入缓冲区的大小
// pub fn lzma_vli_decode(
//     vli: &mut u64,
//     mut vli_pos: Option<&mut usize>,
//     input: &Vec<u8>,
//     in_pos: &mut usize,
//     in_size: usize,
// ) -> LzmaRet {
//     // 处理单次调用模式与流模式
//     let mut vli_pos_internal = 0;

//     match vli_pos {
//         Some(pos) => {
//             if *pos == 0 {
//                 *vli = 0;
//             }
//             if *pos >= LZMA_VLI_BYTES_MAX || (*vli >> *pos * 7 as usize) != 0 {
//                 return LzmaRet::ProgError;
//             }
//             if *in_pos >= in_size {
//                 return LzmaRet::BufError;
//             }
//         }

//         None => {
//             vli_pos = Some(&mut vli_pos_internal);
//             *vli = 0;

//             if *in_pos >= in_size {
//                 return LzmaRet::DataError;
//             }
//         }
//     }

//     loop {
//         // 读取下一个字节
//         let byte = input[*in_pos];
//         *in_pos += 1;

//         // 将读取的字节添加到 vli
//         *vli += ((byte & 0x7F) as u64) << (*vli_pos.as_mut().unwrap()  * 7);
//         *vli_pos.unwrap() += 1;

//         // 检查是否是多字节整数的最后一个字节
//         if (byte & 0x80) == 0 {
//             // 不允许使用可变长度整数作为填充
//             if byte == 0x00 && *vli_pos.unwrap() > 1 {
//                 return LzmaRet::DataError;
//             }

//             return if vli_pos.is_none() {
//                 LzmaRet::Ok
//             } else {
//                 LzmaRet::StreamEnd
//             };
//         }

//         // 检查是否超过最大字节数
//         if *vli_pos.unwrap() == LZMA_VLI_BYTES_MAX {
//             return LzmaRet::DataError;
//         }

//         if *in_pos >= in_size {
//             break;
//         }
//     }

//     if vli_pos.is_none() {
//         LzmaRet::DataError
//     } else {
//         LzmaRet::Ok
//     }
// }

pub fn lzma_vli_decode(
    vli: &mut LzmaVli,
    vli_pos: Option<&mut usize>,
    input: &[u8],
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    // 如果没有提供 vli_pos，使用单次调用模式
    let mut vli_pos_internal = 0;
    let is_internal = vli_pos.is_none();
    let vli_pos = vli_pos.unwrap_or(&mut vli_pos_internal);

    // 初始化 *vli，当开始解码一个新的整数时
    if *vli_pos == 0 {
        *vli = 0;
    }

    // 验证参数
    if *vli_pos >= LZMA_VLI_BYTES_MAX || (*vli >> (*vli_pos * 7)) != 0 {
        return LzmaRet::ProgError;
    }

    // 检查输入
    if *in_pos >= in_size {
        return if is_internal {
            LzmaRet::DataError
        } else {
            LzmaRet::BufError
        };
    }

    loop {
        // 读取下一个字节
        let byte = input[*in_pos];
        *in_pos += 1;

        // 将读取的字节添加到 *vli
        *vli += ((byte & 0x7F) as LzmaVli) << (*vli_pos * 7);
        *vli_pos += 1;

        // 检查是否是多字节整数的最后一个字节
        if (byte & 0x80) == 0 {
            // 不允许使用可变长度整数作为填充，编码必须使用最紧凑的形式
            if byte == 0x00 && *vli_pos > 1 {
                return LzmaRet::DataError;
            }

            return if is_internal {
                LzmaRet::Ok
            } else {
                LzmaRet::StreamEnd
            };
        }

        // 如果已经读取了最大字节数，整数被认为是损坏的
        if *vli_pos == LZMA_VLI_BYTES_MAX {
            return LzmaRet::DataError;
        }

        // 检查是否还有更多输入
        if *in_pos >= in_size {
            break;
        }
    }

    // 如果使用内部 vli_pos，返回 DataError，否则返回 Ok
    if is_internal {
        LzmaRet::DataError
    } else {
        LzmaRet::Ok
    }
}
