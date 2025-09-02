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

#[macro_export]
macro_rules! rc_update_0 {
    ($rc:expr, $prob:expr, $rc_bound:expr) => {
        $rc.range = $rc_bound;
        $prob = (($prob as u32).wrapping_add(
            ((crate::rangecoder::range_common::RC_BIT_MODEL_TOTAL - $prob as u32)
                >> crate::rangecoder::range_common::RC_MOVE_BITS),
        )) as u16;
    };
}

#[macro_export]
macro_rules! rc_update_1 {
    ($rc:expr, $prob:expr, $rc_bound:expr) => {
        $rc.range = $rc.range.wrapping_sub($rc_bound);
        $rc.code = $rc.code.wrapping_sub($rc_bound);
        $prob =
            ($prob as u32 - ($prob as u32 >> crate::rangecoder::range_common::RC_MOVE_BITS)) as u16;
    };
}

#[macro_export]
macro_rules! rc_bit_last {
    ($rc:expr, $prob:expr, $action0:expr, $action1:expr, $seq:expr, $in_pos:expr, $in_size:expr, $input:expr) => {
        if rc_if_0!($rc, $prob, $seq, $in_pos, $in_size, $input) {
            rc_update_0!($rc, $prob, rc_bound);
            $action0;
        } else {
            rc_update_1!($rc, $prob, rc_bound);
            $action1;
        }
    };
}

#[macro_export]
macro_rules! rc_bit {
    ($rc:expr, $prob:expr, $symbol:expr, $action0:expr, $action1:expr, $in_pos:expr, $in_size:expr, $input:expr, $rc_bound:expr,$seq:expr) => {{
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
        if $rc.code < $rc_bound {
            rc_update_0!($rc, $prob, $rc_bound);
            $symbol = ($symbol << 1);
            $action0;
        } else {
            rc_update_1!($rc, $prob, $rc_bound);
            $symbol = ($symbol << 1) + 1;
            $action1;
        }
    }};
}

#[macro_export]
macro_rules! rc_bit_encode {
    ($rc:expr, $prob:expr, $bit:expr) => {
        $rc.symbols[$rc.count] = RcSymbol::try_from($bit).unwrap();
        $rc.probs[$rc.count] = *$prob;
        $rc.count += 1;
    };
}

#[macro_export]
macro_rules! rc_direct {
    ($rc:expr, $dest:expr, $seq:expr, $in_pos:expr, $in_size:expr, $input:expr, $rc_bound:expr) => {
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

        $rc.range >>= 1;
        $rc.code = $rc.code.wrapping_sub($rc.range);
        $rc_bound = 0u32.wrapping_sub($rc.code >> 31);
        $rc.code = $rc.code.wrapping_add($rc.range & $rc_bound);
        $dest = ($dest << 1);
        $dest = $dest.wrapping_add($rc_bound.wrapping_add(1));
    };
}

#[macro_export]
macro_rules! rc_bit_case {
    ($prob:expr, $action0:expr, $action1:expr, $seq:expr) => {
        case $seq:
            rc_bit!($prob, $action0, $action1, $seq);
    };
}
