//! Shared VirtIO HAL backed by a linker-reserved DMA pool.
//!
//! The pool is deliberately separate from the ordinary frame allocator.  VirtIO's
//! `share` contract exposes one physical address for a buffer, so ordinary buffers
//! are staged through this physically-contiguous pool until scatter/gather support
//! is available.

#![allow(clippy::missing_safety_doc)]

use core::{ptr, ptr::NonNull};

use base_config::mm::DMA_POOL_SIZE;
use spin::Mutex;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const DMA_POOL_PAGES: usize = DMA_POOL_SIZE / PAGE_SIZE;
const DMA_POOL_BITMAP_BYTES: usize = (DMA_POOL_PAGES + 7) / 8;
const MAX_SHARED_BUFFERS: usize = 256;

#[repr(align(4096))]
struct DmaStorage([u8; DMA_POOL_SIZE]);

/// Linker places this section before `kernel_end`; ordinary frame allocation starts
/// after it and therefore cannot return these pages.
#[used]
#[unsafe(link_section = ".dma")]
static mut DMA_STORAGE: DmaStorage = DmaStorage([0; DMA_POOL_SIZE]);

#[derive(Clone, Copy)]
struct ShareEntry {
    dma_pa: usize,
    original_ptr: usize,
    len: usize,
    pages: usize,
    direction: BufferDirection,
}

struct DmaState {
    base: usize,
    bitmap: [u8; DMA_POOL_BITMAP_BYTES],
    shares: [Option<ShareEntry>; MAX_SHARED_BUFFERS],
}

impl DmaState {
    const fn new() -> Self {
        Self { base: 0,
               bitmap: [0; DMA_POOL_BITMAP_BYTES],
               shares: [None; MAX_SHARED_BUFFERS] }
    }

    fn ensure_init(&mut self) {
        if self.base != 0 {
            return;
        }
        self.base = unsafe { ptr::addr_of!(DMA_STORAGE.0) as usize };
    }

    #[inline]
    fn used(&self, page: usize) -> bool {
        (self.bitmap[page / 8] & (1 << (page % 8))) != 0
    }

    #[inline]
    fn set_used(&mut self, page: usize, used: bool) {
        let mask = 1 << (page % 8);
        if used {
            self.bitmap[page / 8] |= mask;
        } else {
            self.bitmap[page / 8] &= !mask;
        }
    }

    fn alloc(&mut self, pages: usize) -> Option<(usize, usize)> {
        self.ensure_init();
        if pages == 0 || pages > DMA_POOL_PAGES {
            return None;
        }
        let mut start = 0;
        while start + pages <= DMA_POOL_PAGES {
            if (start..start + pages).all(|page| !self.used(page)) {
                for page in start..start + pages {
                    self.set_used(page, true);
                }
                return Some((self.base + start * PAGE_SIZE, start));
            }
            start += 1;
        }
        None
    }

    fn free(&mut self, start: usize, pages: usize) -> bool {
        if pages == 0 || start >= DMA_POOL_PAGES || start + pages > DMA_POOL_PAGES {
            return false;
        }
        if !(start..start + pages).all(|page| self.used(page)) {
            return false;
        }
        for page in start..start + pages {
            self.set_used(page, false);
        }
        true
    }

    fn find_share(&self, dma_pa: usize) -> Option<(usize, ShareEntry)> {
        self.shares.iter().enumerate().find_map(|(index, entry)| {
            entry.filter(|entry| entry.dma_pa == dma_pa).map(|entry| (index, entry))
        })
    }
}

static DMA_STATE: Mutex<DmaState> = Mutex::new(DmaState::new());

fn pages_for_len(len: usize) -> Option<usize> {
    len.checked_add(PAGE_SIZE - 1).map(|bytes| bytes / PAGE_SIZE)
}

fn pool_alloc(pages: usize) -> Option<usize> {
    DMA_STATE.lock().alloc(pages).map(|(pa, _)| pa)
}

fn pool_free(pa: usize, pages: usize) -> bool {
    let mut state = DMA_STATE.lock();
    state.ensure_init();
    let Some(offset) = pa.checked_sub(state.base) else { return false };
    if offset % PAGE_SIZE != 0 {
        return false;
    }
    state.free(offset / PAGE_SIZE, pages)
}

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let Some(address) = pool_alloc(pages) else {
            return (0, NonNull::dangling());
        };
        unsafe { ptr::write_bytes(address as *mut u8, 0, pages * PAGE_SIZE) };
        let Some(pointer) = NonNull::new(address as *mut u8) else {
            let _ = pool_free(address, pages);
            return (0, NonNull::dangling());
        };
        (address as PhysAddr, pointer)
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        if pages == 0 || paddr == 0 || vaddr.as_ptr() as usize != paddr as usize {
            return -1;
        }
        if pool_free(paddr as usize, pages) { 0 } else { -1 }
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("virtio MMIO address is null")
    }

    unsafe fn share(buffer: NonNull<[u8]>, direction: BufferDirection) -> PhysAddr {
        let len = unsafe { buffer.as_ref().len() };
        let original_ptr = buffer.as_ptr() as *mut u8 as usize;
        let Some(pages) = pages_for_len(len) else { return 0 };
        let Some(dma_pa) = pool_alloc(pages) else { return 0 };

        if matches!(direction, BufferDirection::DriverToDevice | BufferDirection::Both) {
            unsafe {
                ptr::copy_nonoverlapping(original_ptr as *const u8,
                                         dma_pa as *mut u8,
                                         len);
            }
        }

        let mut state = DMA_STATE.lock();
        let Some(slot) = state.shares.iter().position(Option::is_none) else {
            drop(state);
            let _ = pool_free(dma_pa, pages);
            log::error!("[virtio-hal] share metadata exhausted (len={})", len);
            return 0;
        };
        state.shares[slot] = Some(ShareEntry { dma_pa,
                                               original_ptr,
                                               len,
                                               pages,
                                               direction });
        dma_pa as PhysAddr
    }

    unsafe fn unshare(paddr: PhysAddr, buffer: NonNull<[u8]>, direction: BufferDirection) {
        let len = unsafe { buffer.as_ref().len() };
        let original_ptr = buffer.as_ptr() as *mut u8 as usize;
        let entry = {
            let mut state = DMA_STATE.lock();
            let Some((slot, entry)) = state.find_share(paddr as usize) else {
                log::error!("[virtio-hal] unshare unknown dma address={:#x}", paddr);
                return;
            };
            if entry.original_ptr != original_ptr || entry.len != len || entry.direction != direction {
                log::error!("[virtio-hal] unshare metadata mismatch dma={:#x}", paddr);
                return;
            }
            state.shares[slot] = None;
            entry
        };

        if matches!(direction, BufferDirection::DeviceToDriver | BufferDirection::Both) {
            unsafe {
                ptr::copy_nonoverlapping(entry.dma_pa as *const u8,
                                         entry.original_ptr as *mut u8,
                                         entry.len);
            }
        }
        let _ = pool_free(entry.dma_pa, entry.pages);
    }
}
