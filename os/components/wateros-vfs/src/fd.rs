//! per-task fd 会话：全局注册表与当前任务便捷访问。

#![cfg(feature = "fd-session")]

extern crate alloc;

use alloc::boxed::Box;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{VfsError, VfsIoHandle, VfsResult};
use base::sync::UniprocessorSafeCell;
use impl_fd_session::PerTaskFdRegistry;

static mut FD_REGISTRY: MaybeUninit<UniprocessorSafeCell<PerTaskFdRegistry>> = MaybeUninit::uninit();
static FD_REGISTRY_READY: AtomicUsize = AtomicUsize::new(0);

/// 全局 per-task fd 注册表（单核 `UniprocessorSafeCell`）。
pub fn registry() -> &'static UniprocessorSafeCell<PerTaskFdRegistry> {
    if FD_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            FD_REGISTRY.write(UniprocessorSafeCell::new(PerTaskFdRegistry::new()));
        }
        FD_REGISTRY_READY.store(1, Ordering::Release);
    }
    unsafe { &*FD_REGISTRY.as_ptr() }
}

/// 当前任务 id；无运行任务时 [`VfsError::NoTask`]。
pub fn current_task_id() -> VfsResult<task::TaskId> {
    task::current_task_id().ok_or(VfsError::NoTask)
}

/// 在持有注册表锁的情况下执行 `f`（传入可变注册表与当前任务 id）。
pub fn with_current_task<R>(
    f: impl FnOnce(&mut PerTaskFdRegistry, task::TaskId) -> VfsResult<R>,
) -> VfsResult<R> {
    let task_id = current_task_id()?;
    let mut reg = registry().exclusive_access();
    f(&mut reg, task_id)
}

/// 取当前任务下 fd 对应句柄的可变引用（内部已加锁）。
pub fn with_current_io<R>(
    fd: usize,
    f: impl FnOnce(&mut (dyn VfsIoHandle + '_)) -> VfsResult<R>,
) -> VfsResult<R> {
    with_current_task(|reg, task_id| {
        let handle = reg.get_io_for_task(task_id, fd)?;
        f(handle)
    })
}

/// 为当前任务分配 fd。
pub fn alloc_fd(handle: Box<dyn VfsIoHandle>) -> VfsResult<usize> {
    with_current_task(|reg, task_id| Ok(reg.alloc_fd_for_task(task_id, handle)))
}

/// 关闭当前任务的 fd（调用句柄 `close`）。
pub fn close_fd(fd: usize) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.close_fd_for_task(task_id, fd))
}

/// bring-up：两任务 fd 表隔离与 close 语义烟囱。
pub fn self_test() {
    let mut reg = registry().exclusive_access();
    let a: task::TaskId = 10;
    let b: task::TaskId = 11;
    let fd = reg.alloc_fd_for_task(a, Box::new(impl_fd_session::ConsoleOutHandle));
    let fd_b = reg.alloc_fd_for_task(b, Box::new(impl_fd_session::ConsoleOutHandle));
    // 各任务独立 fd 表，首个动态 fd 号可以相同，隔离体现在句柄互不影响。
    assert_eq!(fd, fd_b);
    assert!(reg.get_io_for_task(a, fd).is_ok());
    assert!(reg.get_io_for_task(b, fd_b).is_ok());
    assert!(reg.close_fd_for_task(a, fd).is_ok());
    assert!(reg.get_io_for_task(a, fd).is_err());
    assert!(reg.get_io_for_task(b, fd_b).is_ok());
    assert!(reg.close_fd_for_task(a, fd).is_err());
    let fd_reuse = reg.alloc_fd_for_task(a, Box::new(impl_fd_session::ConsoleOutHandle));
    assert_eq!(fd_reuse, fd);
    let _ = reg.close_fd_for_task(b, fd_b);
}
