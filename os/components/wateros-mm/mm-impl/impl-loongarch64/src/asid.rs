//! LoongArch 用户地址空间 ASID 分配与硬件 token 编码。
//!
//! ASID 0 保留给内核地址空间。用户 ASID 只有在调用方确认所有可能缓存该
//! ASID 的 CPU 已完成 TLB 失效后才能归还，避免复用时命中上一地址空间的映射。

use api_v0::error::{MmError, MmResult};
use spin::Mutex;

pub(crate) const KERNEL_ASID : u16 = 0;
pub(crate) const ASID_BITS : usize = 10;
pub(crate) const ASID_COUNT : usize = 1 << ASID_BITS;
pub(crate) const ASID_MASK : usize = ASID_COUNT - 1;

pub(crate) const TOKEN_ASID_SHIFT : usize = 48;
pub(crate) const TOKEN_PGDL_MASK : usize = (1usize << TOKEN_ASID_SHIFT) - 1;

struct AsidAllocator {
    allocated : [u64; ASID_COUNT / u64::BITS as usize],
}

impl AsidAllocator {
    const fn new() -> Self { Self { allocated : [0; ASID_COUNT / u64::BITS as usize] } }

    fn allocate(&mut self) -> Option<u16> {
        // ASID 0 永不分配给用户地址空间。
        for raw in 1..ASID_COUNT {
            let word = raw / u64::BITS as usize;
            let bit = 1u64 << (raw % u64::BITS as usize);
            if self.allocated[word] & bit == 0 {
                self.allocated[word] |= bit;
                return Some(raw as u16);
            }
        }
        None
    }

    fn release(&mut self, asid : u16) {
        let raw = asid as usize;
        if raw == KERNEL_ASID as usize || raw >= ASID_COUNT {
            return;
        }
        let word = raw / u64::BITS as usize;
        let bit = 1u64 << (raw % u64::BITS as usize);
        debug_assert_ne!(self.allocated[word] & bit, 0, "releasing an unused LoongArch ASID");
        self.allocated[word] &= !bit;
    }
}

static USER_ASIDS : Mutex<AsidAllocator> = Mutex::new(AsidAllocator::new());

pub(crate) fn allocate_user() -> MmResult<u16> {
    USER_ASIDS.lock()
              .allocate()
              .ok_or(MmError::OutOfMemory)
}

/// 调用方必须先使所有缓存过该 ASID 的 CPU 完成 TLB 失效。
pub(crate) fn release_user(asid : u16) { USER_ASIDS.lock().release(asid); }

#[inline]
pub(crate) const fn encode_token(pgdl : usize, asid : u16) -> usize {
    (pgdl & TOKEN_PGDL_MASK) | (((asid as usize) & ASID_MASK) << TOKEN_ASID_SHIFT)
}
