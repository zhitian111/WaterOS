#![no_std]
use buddy_system_allocator::LockedHeap;
use config::mm::{KERNEL_HEAP_SIZE, KERNEL_HEAP_SIZE_BIT_WIDTH};
use core::ptr::addr_of_mut;


#[allow(unused)]
#[global_allocator]
#[cfg(feature = "impl-buddy-allocator")]
static HEAP_ALLOCATOR : LockedHeap<KERNEL_HEAP_SIZE_BIT_WIDTH> = LockedHeap::new();
pub fn handle_alloc_error(layout : core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}",
           layout);
}

#[allow(unused)]
#[link_name = "kernel_heap"]
static mut HEAP_SPACE : [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init() {
    unsafe {
        HEAP_ALLOCATOR.lock()
                      .init(addr_of_mut!(HEAP_SPACE) as usize,
                            KERNEL_HEAP_SIZE);
    }
}
