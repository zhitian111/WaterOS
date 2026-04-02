#![no_std]

pub mod addr;
pub mod error;
pub mod perm;
pub mod flags;

pub mod frame_allocator;
pub mod address_space;
pub mod user_access;

pub mod brk;
pub mod mmap;

pub use frame_allocator::PhysicalFrameAllocator;

pub fn test() {
    log::trace!("[mm-api] test begin");
    addr::test();
    perm::test();
    flags::test();
    log::trace!("[mm-api] test end");
}
