//! `brk(2)`：有效 `user_aspace_ptr` 时走 Sv39 `HeapBrk`，否则单调递增假顶桩。

use abi::user_ret::UserRet;
use core::sync::atomic::Ordering;

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno, USER_BRK_FAKE};

fn sys_brk_mm(handle: usize, addr: usize) -> UserRet {
    use mm::api::addr::VirtAddr;
    use mm::api::brk::HeapBrk;
    use mm::frame_alloctor::GlobalPhysFrameAllocator;
    match mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
        let mut alloc = GlobalPhysFrameAllocator;
        if addr == 0 {
            return Ok(HeapBrk::brk_region(aspace)
                .current_end
                .0);
        }
        match HeapBrk::brk(aspace, &mut alloc, VirtAddr(addr)) {
            Ok(new) => Ok(new.0),
            Err(e) => Err(e),
        }
    }) {
        Ok(v) => UserRet::from_success(v),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

fn sys_brk_fake(addr: usize) -> UserRet {
    // 须高于静态链接用户镜像末端（含大 `.bss` 堆）；仅作 `brk(0)` 查询桩。
    const INITIAL: usize = 0x0120_0000;
    if addr == 0 {
        let v = USER_BRK_FAKE.load(Ordering::Relaxed);
        if v == 0 {
            USER_BRK_FAKE.store(INITIAL, Ordering::Relaxed);
            return UserRet::from_success(INITIAL);
        }
        return UserRet::from_success(v);
    }
    let cur = USER_BRK_FAKE
        .load(Ordering::Relaxed)
        .max(INITIAL);
    if addr < cur {
        return UserRet::from_success(cur);
    }
    USER_BRK_FAKE.store(addr, Ordering::Relaxed);
    UserRet::from_success(addr)
}

pub(crate) fn sys_brk(addr: usize) -> UserRet {
    if let Some(handle) = current_user_aspace_handle() {
        return sys_brk_mm(handle, addr);
    }
    sys_brk_fake(addr)
}
