/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

use crate::common::common::Sequence;
use crate::common::{lzma_next_end, LzmaInternal, LZMA_ACTION_MAX};
use std::cell::{Cell, RefCell};

pub type LzmaBool = u8;
pub type LzmaReservedEnum = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LzmaRet {
    Ok = 0,
    StreamEnd = 1,
    NoCheck = 2,
    UnsupportedCheck = 3,
    GetCheck = 4,
    MemError = 5,
    MemlimitError = 6,
    FormatError = 7,
    OptionsError = 8,
    DataError = 9,
    BufError = 10,
    ProgError = 11,
    SeekNeeded = 12,
    RetInternal1 = 13,
}

// #[derive(Debug)]
// pub struct LzmaAllocator {
//     pub alloc: Option<fn(opaque: *mut (), size: usize) -> *mut ()>,
//     pub free: Option<fn(opaque: *mut (), ptr: *mut ())>,
//     pub opaque: Box<()>,
// }

// impl Default for LzmaAllocator {
//     fn default() -> Self {
//         Self {
//             alloc: None,
//             free: None,
//             opaque: Box::new(()),
//         }
//     }
// }

#[derive(Debug)]
pub struct LzmaStream<'a> {
    // 输入输出缓冲区
    pub next_in: &'a [u8],
    pub avail_in: Cell<usize>,
    pub total_in: Cell<u64>,

    pub next_out: RefCell<Vec<u8>>,
    pub avail_out: Cell<usize>,
    pub total_out: Cell<u64>,
    pub next_out_pos: u32,

    // 内部状态管理
    // pub allocator: &'a LzmaAllocator,
    pub internal: RefCell<Option<LzmaInternal>>,

    // 其他状态
    pub seek_pos: Cell<u64>,
    pub reserved_int2: Cell<u64>,
    pub reserved_int3: Cell<usize>,
    pub reserved_int4: Cell<usize>,
    pub reserved_enum1: Cell<LzmaReservedEnum>,
    pub reserved_enum2: Cell<LzmaReservedEnum>,
}

impl<'a> Default for LzmaStream<'a> {
    fn default() -> Self {
        Self {
            next_in: &[],
            avail_in: Cell::new(0),
            total_in: Cell::new(0),
            next_out: RefCell::new(Vec::new()),
            avail_out: Cell::new(0),
            total_out: Cell::new(0),
            // allocator: unsafe { std::mem::zeroed() },
            internal: RefCell::new(None),
            seek_pos: Cell::new(0),
            reserved_int2: Cell::new(0),
            reserved_int3: Cell::new(0),
            reserved_int4: Cell::new(0),
            reserved_enum1: Cell::new(0),
            reserved_enum2: Cell::new(0),
            next_out_pos: 0,
        }
    }
}

impl<'a> LzmaStream<'a> {
    // 提供安全的接口方法
    pub fn init(&self) -> LzmaRet {
        let mut internal = self.internal.borrow_mut();
        if internal.is_none() {
            *internal = Some(LzmaInternal::new());
        }

        if let Some(int) = internal.as_mut() {
            int.supported_actions = [false; LZMA_ACTION_MAX + 1];
            int.sequence = Sequence::Run;
            int.allow_buf_error = false;
        }

        self.total_in.set(0);
        self.total_out.set(0);

        LzmaRet::Ok
    }

    pub fn end(&self) {
        let mut internal = self.internal.borrow_mut();
        if let Some(int) = internal.as_mut() {
            if let Some(next) = int.next.as_mut() {
                lzma_next_end(next);
                int.next = None;
            }
        }
    }

    pub fn get_internal_mut(&self) -> Option<std::cell::RefMut<LzmaInternal>> {
        if self.internal.borrow().is_some() {
            Some(std::cell::RefMut::map(self.internal.borrow_mut(), |i| {
                i.as_mut().unwrap()
            }))
        } else {
            None
        }
    }
}

#[macro_export]
macro_rules! LZMA_STREAM_INIT {
    () => {
        LzmaStream {
            next_in: &mut Vec::new(),
            avail_in: 0,
            total_in: 0,
            next_out: &mut Vec::new(),
            avail_out: 0,
            total_out: 0,
            allocator: &mut LzmaAllocator {
                alloc: None,
                free: None,
                opaque: Box::new(()),
            },
            internal: None,
            reserved_ptr1: Box::new(()),
            reserved_ptr2: Box::new(()),
            reserved_ptr3: Box::new(()),
            reserved_ptr4: Box::new(()),
            seek_pos: 0,
            reserved_int2: 0,
            reserved_int3: 0,
            reserved_int4: 0,
            reserved_enum1: LzmaReservedEnum::Reserved,
            reserved_enum2: LzmaReservedEnum::Reserved,
        };
    };
}
