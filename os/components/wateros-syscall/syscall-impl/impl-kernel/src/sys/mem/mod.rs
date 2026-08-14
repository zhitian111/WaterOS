//! 内存管理相关的 syscall 实现。

pub(crate) mod brk;
pub(crate) mod mempolicy;
pub(crate) mod mmap;
pub(crate) mod mincore;

pub(crate) use brk::sys_brk;
pub(crate) use mempolicy::sys_get_mempolicy;
pub(crate) use mmap::{
    sys_madvise, sys_mlock, sys_mlockall, sys_mmap, sys_mprotect, sys_mremap, sys_msync,
    sys_munlock, sys_munlockall, sys_munmap,
};
pub(crate) use mincore::sys_mincore;
