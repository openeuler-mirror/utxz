/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::api::LzmaRet;

#[derive(Clone, Copy, Default, Debug)]
pub struct LzmaRangeDecoder {
    pub range: u32,
    pub code: u32,
    pub init_bytes_left: u32,
}

pub fn rc_read_init(
    rc: &mut LzmaRangeDecoder,
    in_: &Vec<u8>,
    in_pos: &mut usize,
    in_size: usize,
) -> LzmaRet {
    while rc.init_bytes_left > 0 {
        if *in_pos == in_size {
            return LzmaRet::Ok;
        }

        if rc.init_bytes_left == 5 && in_[*in_pos] != 0x00 {
            return LzmaRet::DataError;
        }

        rc.code = (rc.code << 8) | in_[*in_pos] as u32;
        *in_pos += 1;
        rc.init_bytes_left = rc.init_bytes_left.wrapping_sub(1);
    }

    LzmaRet::StreamEnd
}

// 宏定义
#[macro_export]
macro_rules! rc_to_local {
    ($range_decoder:expr, $in_pos:expr) => {
        let mut rc = $range_decoder.clone();
        let mut rc_in_pos = $in_pos;
        let mut rc_bound: u32;
    };
}

#[macro_export]
macro_rules! rc_from_local {
    ($range_decoder:expr, $in_pos:expr) => {
        *$range_decoder = rc;
        *$in_pos = rc_in_pos;
    };
}

#[macro_export]
macro_rules! rc_reset {
    ($range_decoder:expr) => {
        $range_decoder.range = u32::MAX;
        $range_decoder.code = 0;
        $range_decoder.init_bytes_left = 5;
    };
}

#[macro_export]
macro_rules! rc_is_finished {
    ($range_decoder:expr) => {
        $range_decoder.code == 0
    };
}

#[macro_export]
macro_rules! rc_normalize {
    ($rc:expr, $seq:expr, $in_pos:expr, $in_size:expr, $input:expr) => {
        if $rc.range < crate::rangecoder::range_common::RC_TOP_VALUE {
            if $in_pos == $in_size {
                $seq;
                break;
            }
            $rc.range <<= crate::rangecoder::range_common::RC_SHIFT_BITS;
            $rc.code = ($rc.code << crate::rangecoder::range_common::RC_SHIFT_BITS)
                | ($input[$in_pos] as u32);
            $in_pos += 1;
        }
    };
}

#[macro_export]
macro_rules! rc_if_0 {
    ($rc:expr, $prob:expr, $seq:expr, $in_pos:expr, $in_size:expr, $input:expr, $rc_bound:expr) => {{
        if $rc.range < crate::rangecoder::range_common::RC_TOP_VALUE {
            if $in_pos == $in_size {
                $seq;
                break;
            }
            $rc.range <<= crate::rangecoder::range_common::RC_SHIFT_BITS;
            $rc.code = ($rc.code << crate::rangecoder::range_common::RC_SHIFT_BITS)
                | ($input[$in_pos] as u32);
            $in_pos += 1;
        }
        $rc_bound = ($rc.range >> crate::rangecoder::range_common::RC_BIT_MODEL_TOTAL_BITS)
            .wrapping_mul($prob as u32);
        $rc.code < $rc_bound
    }};
}
