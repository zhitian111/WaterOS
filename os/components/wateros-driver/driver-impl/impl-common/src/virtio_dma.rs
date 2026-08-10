//! Shared VirtIO DMA allocation boundary.
//!
//! `virtio-drivers` exposes raw address-based callbacks, so this module keeps
//! the checked contiguous-frame allocation and identity-mapping assumptions in
//! one place.  Cache maintenance is deliberately not implemented here.

#![allow(unsafe_op_in_unsafe_fn)]

use core::ptr::NonNull;

use frame_alloctor::{frame_alloc_contiguous, frame_dealloc_contiguous, FrameSpan};
use mm_api::addr::{PhysPageNum, PAGE_SIZE};

/// Checked byte length for a VirtIO page request.
pub const fn byte_len(pages : usize) -> Option<usize> { pages.checked_mul(PAGE_SIZE) }

/// Allocate zeroed physically contiguous pages under the frame allocator.
///
/// The returned pointer is valid only under the current kernel identity-map
/// bring-up contract; callers must not infer cache coherency from this helper.
pub fn alloc(pages : usize) -> Option<(u64, NonNull<u8>)> {
    let length = byte_len(pages)?;
    if pages == 0 {
        return None;
    }
    let span = frame_alloc_contiguous(pages, 1).ok()?;
    let address = span.start()
                      .start_addr()
                      .0;
    let Some(pointer) = NonNull::new(address as *mut u8) else {
        let _ = frame_dealloc_contiguous(span);
        return None;
    };
    unsafe {
        core::ptr::write_bytes(pointer.as_ptr(), 0, length);
    }
    Some((address as u64, pointer))
}

/// Release a VirtIO allocation, rejecting non-identity pointers and malformed
/// spans instead of silently leaking or freeing an unrelated frame range.
pub unsafe fn dealloc(address : u64, pointer : NonNull<u8>, pages : usize) -> i32 {
    let Some(length) = byte_len(pages) else {
        return -1;
    };
    if pages == 0 ||
       address == 0 ||
       address % PAGE_SIZE as u64 != 0 ||
       pointer.as_ptr() as usize != address as usize ||
       length == 0
    {
        return -1;
    }
    let Ok(base) = usize::try_from(address) else {
        return -1;
    };
    let span = FrameSpan::new(PhysPageNum(base / PAGE_SIZE), pages);
    frame_dealloc_contiguous(span).is_ok()
                                  .then_some(0)
                                  .unwrap_or(-1)
}

/// Convert an identity-mapped buffer pointer to a device address.
pub unsafe fn share_identity(buffer : NonNull<[u8]>) -> u64 {
    buffer.as_ptr() as *mut u8 as usize as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_lengths_and_invalid_dealloc_inputs_fail_closed() {
        assert_eq!(byte_len(0), Some(0));
        assert_eq!(byte_len(2), Some(PAGE_SIZE * 2));
        assert_eq!(byte_len(usize::MAX), None);
        let pointer = NonNull::dangling();
        assert_eq!(unsafe { dealloc(0x1000, pointer, 1) },
                   -1);
        assert_eq!(unsafe { dealloc(0x1001, pointer, 1) },
                   -1);
        assert_eq!(unsafe { dealloc(0x1000, pointer, 0) },
                   -1);
    }
}
