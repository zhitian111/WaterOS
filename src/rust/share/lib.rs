#![no_std]
pub mod def;
pub mod fs;
pub mod io;
//pub mod + 文件夹名字

use linked_list_allocator::*;
#[global_allocator]
static ALLOCATOR : LockedHeap = LockedHeap::empty();

pub fn init_allocator(heap_start : *mut u8, heap_size : usize) {
    unsafe {
        ALLOCATOR.lock()
                 .init(heap_start as *mut u8, heap_size);
    }
}
