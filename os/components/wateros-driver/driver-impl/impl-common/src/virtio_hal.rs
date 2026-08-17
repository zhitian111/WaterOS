//! Shared identity-mapped DMA HAL for all virtio transports.
#![allow(clippy::missing_safety_doc)]

use alloc::vec::Vec;
use core::{ptr, ptr::NonNull};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages : usize, _direction : BufferDirection) -> (PhysAddr, NonNull<u8>) {
        if pages == 0 {
            return (0, NonNull::dangling());
        }
        let mut ppns = Vec::new();
        for _ in 0..pages {
            match frame_alloc_result() {
                Ok(ppn) => ppns.push(ppn),
                Err(_) => {
                    for ppn in ppns {
                        let _ = frame_dealloc_result(ppn);
                    }
                    return (0, NonNull::dangling());
                }
            }
        }
        if (1..pages).any(|i| ppns[i - 1].0 != ppns[i].0 + 1) {
            for ppn in ppns {
                let _ = frame_dealloc_result(ppn);
            }
            return (0, NonNull::dangling());
        }
        let Some(address) = ppns[pages - 1].0
                                           .checked_mul(PAGE_SIZE)
        else {
            for ppn in ppns {
                let _ = frame_dealloc_result(ppn);
            }
            return (0, NonNull::dangling());
        };
        let ptr = address as *mut u8;
        unsafe {
            ptr::write_bytes(ptr, 0, pages * PAGE_SIZE);
        }
        (address as PhysAddr, NonNull::new(ptr).unwrap_or(NonNull::dangling()))
    }
    unsafe fn dma_dealloc(paddr : PhysAddr, _vaddr : NonNull<u8>, pages : usize) -> i32 {
        let base = paddr as usize / PAGE_SIZE;
        for i in 0..pages {
            let _ = frame_dealloc_result(PhysPageNum(base + i));
        }
        0
    }
    unsafe fn mmio_phys_to_virt(paddr : PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("virtio MMIO address is null")
    }
    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }
    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}
