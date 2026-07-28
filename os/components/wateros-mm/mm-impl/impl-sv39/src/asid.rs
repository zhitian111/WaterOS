//! Sv39 用户地址空间 ASID 分配。
//!
//! ASID 0 保留给内核。硬件不实现 ASID 时，所有用户地址空间也使用 0，trap
//! 路径继续执行全量 `sfence.vma`。实现 ASID 时，编号只有在所有可能缓存过它
//! 的 hart 完成 TLB 失效后才能归还。

use api_v0::error::{MmError, MmResult};
use spin::Mutex;

pub(crate) const KERNEL_ASID : u16 = 0;
const MAX_ASID_BITS : usize = 16;
const MAX_ASID_COUNT : usize = 1 << MAX_ASID_BITS;
pub(crate) const TOKEN_ASID_SHIFT : usize = 44;
pub(crate) const TOKEN_ASID_MASK : usize = MAX_ASID_COUNT - 1;

#[inline]
pub(crate) const fn from_token(token : usize) -> u16 {
    ((token >> TOKEN_ASID_SHIFT) & TOKEN_ASID_MASK) as u16
}

struct AsidAllocator {
    allocated : [u64; MAX_ASID_COUNT / u64::BITS as usize],
    limit : usize,
}

impl AsidAllocator {
    const fn new() -> Self {
        Self { allocated : [0; MAX_ASID_COUNT / u64::BITS as usize],
               limit : MAX_ASID_COUNT }
    }

    fn initialize(&mut self, implemented_bits : usize) {
        let bits = implemented_bits.min(MAX_ASID_BITS);
        self.limit = if bits == 0 { 1 } else { 1usize << bits };
    }

    fn allocate(&mut self) -> Option<u16> {
        // ASIDLEN=0 时，trap 路径用全量 fence 保证地址空间切换正确。
        if self.limit == 1 {
            return Some(KERNEL_ASID);
        }
        for raw in 1..self.limit {
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
        if raw == KERNEL_ASID as usize || raw >= self.limit {
            return;
        }
        let word = raw / u64::BITS as usize;
        let bit = 1u64 << (raw % u64::BITS as usize);
        debug_assert_ne!(self.allocated[word] & bit, 0, "releasing an unused RISC-V ASID");
        self.allocated[word] &= !bit;
    }
}

static USER_ASIDS : Mutex<AsidAllocator> = Mutex::new(AsidAllocator::new());

pub(crate) fn initialize(implemented_bits : usize) {
    USER_ASIDS.lock()
              .initialize(implemented_bits);
}

pub(crate) fn allocate_user() -> MmResult<u16> {
    USER_ASIDS.lock()
              .allocate()
              .ok_or(MmError::OutOfMemory)
}

/// 调用方必须先使所有缓存过该 ASID 的 hart 完成 TLB 失效。
pub(crate) fn release_user(asid : u16) { USER_ASIDS.lock().release(asid); }

#[cfg(test)]
mod tests {
    use super::AsidAllocator;

    #[test]
    fn allocate_and_reuse_with_small_hardware_space() {
        let mut allocator = AsidAllocator::new();
        allocator.initialize(2);
        assert_eq!(allocator.allocate(), Some(1));
        assert_eq!(allocator.allocate(), Some(2));
        assert_eq!(allocator.allocate(), Some(3));
        assert_eq!(allocator.allocate(), None);
        allocator.release(2);
        assert_eq!(allocator.allocate(), Some(2));
    }

    #[test]
    fn no_hardware_asid_uses_zero_with_fence_fallback() {
        let mut allocator = AsidAllocator::new();
        allocator.initialize(0);
        assert_eq!(allocator.allocate(), Some(0));
        assert_eq!(allocator.allocate(), Some(0));
    }
}
