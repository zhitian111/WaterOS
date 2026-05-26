//! 系统调用层与 [`wateros-mm`] API 之间的错误与标志转换。

use crate::unsupported::syscall_unsupported;
use abi::errno::ErrNo;

/// 用户态 `brk` 的单调递增假顶：在无 ELF
/// 用户页表（`user_aspace_ptr==0`）时兜底。
pub(crate) static USER_BRK_FAKE : core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

pub(crate) fn mm_err_to_errno(e : mm::api::error::MmError) -> ErrNo {
    use mm::api::error::MmError;
    match e {
        MmError::OutOfMemory | MmError::FrameAlloc(_) => ErrNo::ENOMEM,
        MmError::InvalidAddress | MmError::AlreadyMapped | MmError::NotMapped => ErrNo::EINVAL,
        MmError::AccessViolation => ErrNo::EFAULT,
        MmError::Unsupported => syscall_unsupported("mm"),
    }
}

pub(crate) fn linux_mmap_prot_to_perm(prot : i32) -> mm::api::perm::PagePerm {
    use mm::api::perm::PagePerm;
    let mut p = PagePerm::empty();
    if prot & 1 != 0 {
        p |= PagePerm::R;
    }
    if prot & 2 != 0 {
        p |= PagePerm::W;
    }
    if prot & 4 != 0 {
        p |= PagePerm::X;
    }
    p
}

pub(crate) fn linux_mmap_flags_to_map_flags(flags : u32) -> mm::api::flags::MapFlags {
    use mm::api::flags::MapFlags;
    const MAP_SHARED : u32 = 0x01;
    const MAP_PRIVATE : u32 = 0x02;
    const MAP_FIXED : u32 = 0x10;
    const MAP_ANONYMOUS : u32 = 0x20;
    let mut mf = MapFlags::empty();
    if flags & MAP_SHARED != 0 {
        mf |= MapFlags::SHARED;
    }
    if flags & MAP_PRIVATE != 0 {
        mf |= MapFlags::PRIVATE;
    }
    if flags & MAP_ANONYMOUS != 0 {
        mf |= MapFlags::ANONYMOUS;
    }
    if flags & MAP_FIXED != 0 {
        mf |= MapFlags::FIXED;
    }
    mf
}

/// Linux `mmap` 是否带 `MAP_ANONYMOUS`。
#[inline]
pub(crate) fn linux_mmap_is_anonymous(flags : u32) -> bool {
    const MAP_ANONYMOUS : u32 = 0x20;
    flags & MAP_ANONYMOUS != 0
}

pub(crate) fn current_user_aspace_handle() -> Option<usize> {
    let p = task::current_task_user_aspace_ptr();
    if p == 0 {
        None
    } else {
        Some(p)
    }
}
