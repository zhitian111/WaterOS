//! 由链接器预留 DMA 池支持的共享 VirtIO HAL。
//!
//! DMA 池与普通帧分配器隔离。VirtIO 的 `share` 契约只暴露一个物理地址，
//! 因此在支持散布/聚集前，普通缓冲区必须先复制到物理连续的暂存区。

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

/// 链接器把该段放在 `kernel_end` 之前；普通帧分配从其后开始，不会返回这些页。
#[used]
#[unsafe(link_section = ".dma")]
static mut DMA_STORAGE: DmaStorage = DmaStorage([0; DMA_POOL_SIZE]);

#[derive(Clone, Copy)]
struct ShareEntry {
    /// DMA 池中的物理地址。
    dma_pa: usize,
    /// 原始 CPU 缓冲区地址。
    original_ptr: usize,
    /// 原始缓冲区长度（字节）。
    len: usize,
    /// 占用的 DMA 页数。
    pages: usize,
    /// 共享方向，用于决定复制入/复制出。
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
        // 零页没有可表示的缓冲区；超过池容量直接失败，避免区间计算溢出。
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
        // 驱动必须传回原始地址和页数；任一不匹配都拒绝释放，防止破坏位图。
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
        // 长度向页数上取整；加法溢出或池耗尽均返回 0，调用方应转为 I/O 错误。
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
                // 未知地址不能释放或复制，避免重复 unshare 造成双重释放。
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
