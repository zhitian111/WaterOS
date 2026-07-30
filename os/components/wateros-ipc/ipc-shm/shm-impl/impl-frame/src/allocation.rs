//! SHM 段物理帧分配与释放。

use alloc::vec::Vec;
use api_v0::{PhysPageNum, ShmError, ShmResult};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PAGE_SIZE;

/// `INVARIANT:` 将用户请求转换为非零、页对齐段大小，所有创建/映射路径共享此规则。
pub(crate) fn round_up_pages(size: usize) -> ShmResult<usize> {
    size.checked_add(PAGE_SIZE - 1)
        .map(|value| value / PAGE_SIZE * PAGE_SIZE)
        .ok_or(ShmError::Invalid)
}

/// `FLOW:` 原子地分配并清零段的全部帧；中途失败会回滚已分配帧。
pub(crate) fn alloc_segment_pages(size: usize) -> ShmResult<Vec<PhysPageNum>> {
    let count = round_up_pages(size)? / PAGE_SIZE;
    let mut pages = Vec::new();
    for _ in 0..count {
        let page = match frame_alloc_result() {
            Ok(page) => page,
            Err(_) => {
                for allocated in pages {
                    let _ = frame_dealloc_result(allocated);
                }
                return Err(ShmError::NoMem);
            }
        };
        zero_page(page);
        pages.push(page);
    }
    Ok(pages)
}

/// `UNSAFE:` 该物理帧已由 allocator 独占分配，且在发布到 segment 前被清零。
fn zero_page(page: PhysPageNum) {
    let addr = page.start_addr().0 as *mut u8;
    unsafe { core::ptr::write_bytes(addr, 0, PAGE_SIZE) }
}
