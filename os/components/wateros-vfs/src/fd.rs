//! per-task fd 会话：全局注册表与当前任务便捷访问。

#![cfg(feature = "impl-fd-session")]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{VfsError, VfsIoHandle, VfsResult};
use base::sync::UniprocessorSafeCell;
use impl_fd_session::PerTaskFdRegistry;

static mut FD_REGISTRY : MaybeUninit<UniprocessorSafeCell<PerTaskFdRegistry>> =
    MaybeUninit::uninit();
static FD_REGISTRY_READY : AtomicUsize = AtomicUsize::new(0);

/// 全局 per-task fd 注册表（单核 `UniprocessorSafeCell`）。
#[inline]
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
#[inline]
pub fn current_task_id() -> VfsResult<task::TaskId> {
    task::current_task_id().ok_or(VfsError::NoTask)
}

/// 在关中断临界区内访问 fd 注册表。
fn with_fd_registry<R>(f: impl FnOnce(&mut PerTaskFdRegistry) -> VfsResult<R>) -> VfsResult<R> {
    impl_fd_session::with_interrupt_disabled(|| f(&mut registry().exclusive_access()))
}

/// 在持有注册表锁的情况下执行 `f`（传入可变注册表与当前任务 id）。
#[inline]
pub fn with_current_task<R>(f : impl FnOnce(&mut PerTaskFdRegistry, task::TaskId) -> VfsResult<R>)
                            -> VfsResult<R> {
    let task_id = current_task_id()?;
    with_fd_registry(|reg| f(reg, task_id))
}

/// 取当前任务下 fd 对应句柄的可变引用（内部已加锁）。
pub fn with_current_io<R>(fd : usize,
                          f : impl FnOnce(&mut (dyn VfsIoHandle + '_)) -> VfsResult<R>)
                          -> VfsResult<R> {
    let task_id = current_task_id()?;
    let shared =
        with_fd_registry(|reg| Ok(reg.is_fd_table_shared(task_id)))?;

    if !shared {
        let mut handle = with_fd_registry(|reg| reg.take_io_for_task(task_id, fd))?;
        let result = f(handle.as_mut());
        let restore_result =
            with_fd_registry(|reg| reg.restore_io_for_task(task_id, fd, handle));
        if let Err(ref e) = restore_result {
            log::warn!("[vfs-fd] with_current_io task_id={:?} fd={} restore_failed: {:?}",
                       task_id,
                       fd,
                       e);
        }
        restore_result?;
        return result;
    }

    let (handle_ptr, slot_lock) =
        with_fd_registry(|reg| reg.begin_shared_io_for_task(task_id, fd))?;
    let _slot_guard = slot_lock.lock();
    let result = f(unsafe {
        // SAFETY: inflight 计数与槽位锁保证句柄驻留且互斥访问。
        &mut *handle_ptr
    });
    with_fd_registry(|reg| {
        reg.end_shared_io_for_task(task_id, fd);
        Ok(())
    })?;
    result
}

/// 为当前任务分配 fd。
#[inline]
pub fn alloc_fd(handle : Box<dyn VfsIoHandle>) -> VfsResult<usize> {
    with_current_task(|reg, task_id| reg.alloc_fd_for_task(task_id, handle))
}

// 本方法代码由AI完成
fn release_locks_for_current_process(handle : &(dyn VfsIoHandle + '_)) {
    let Some(pid) = task::current_process_task_snapshot().map(|snap| snap.pid) else {
        return;
    };
    let Ok(meta) = handle.metadata() else {
        return;
    };
    let Some(key) = inode_key_from_metadata(&meta) else {
        return;
    };
    release_process_inode_locks(pid, &key);
    if let Some(owner) = handle.flock_owner_id() {
        release_flock_owner(&key, owner);
    }
}

/// 关闭当前任务的 fd（调用句柄 `close`）。
#[inline]
pub fn close_fd(fd : usize) -> VfsResult<()> {
    let task_id = current_task_id()?;
    let mut handle = with_fd_registry(|reg| reg.take_fd_for_close(task_id, fd))?;
    release_locks_for_current_process(handle.as_ref());
    handle.close()
}

/// 关闭当前任务 fd 区间内所有已打开 fd；未打开 fd 按 Linux `close_range` 语义忽略。
pub fn close_fd_range(first : usize, last : usize) -> VfsResult<Vec<usize>> {
    let task_id = current_task_id()?;
    let handles = with_fd_registry(|reg| reg.take_fd_range_for_close(task_id, first, last))?;
    let mut closed = Vec::new();
    for (fd, mut handle) in handles {
        release_locks_for_current_process(handle.as_ref());
        handle.close()?;
        closed.push(fd);
    }
    Ok(closed)
}

/// 请求全部打开句柄写回脏数据。
pub fn flush_all_open_files() -> VfsResult<()> {
    registry().exclusive_access()
              .flush_all()
}

/// 当前任务下 `fd` 是否为 TTY 类字符设备。
pub fn current_fd_is_tty_char(fd : usize) -> VfsResult<bool> {
    with_current_task(|reg, task_id| {
        let handle = reg.get_io_for_task(task_id, fd)?;
        Ok(handle.is_tty_char_device())
    })
}

/// 当前任务下 `fd` 是否为软件 RTC 字符设备。
pub fn current_fd_is_rtc(fd : usize) -> VfsResult<bool> {
    with_current_task(|reg, task_id| {
        let handle = reg.get_io_for_task(task_id, fd)?;
        if handle.is_rtc_device() {
            return Ok(true);
        }
        Ok(handle.metadata()
                 .map(|m| m.mode == 0o20644)
                 .unwrap_or(false))
    })
}

/// `dup(oldfd)`：复制到 ≥ `minfd` 的最低可用 fd。
#[inline]
pub fn dup_fd(oldfd : usize, minfd : usize) -> VfsResult<usize> {
    with_current_task(|reg, task_id| reg.dup_fd_for_task(task_id, oldfd, minfd))
}

/// `dup3(oldfd, newfd, cloexec)`。
#[inline]
pub fn dup3_fd(oldfd : usize, newfd : usize, cloexec : bool) -> VfsResult<usize> {
    with_current_task(|reg, task_id| reg.dup3_fd_for_task(task_id, oldfd, newfd, cloexec))
}

/// `fcntl(F_GETFD)`。
#[inline]
pub fn get_fd_flags(fd : usize) -> VfsResult<usize> {
    with_current_task(|reg, task_id| reg.get_fd_flags(task_id, fd))
}

/// `fcntl(F_SETFD)`。
#[inline]
pub fn set_fd_flags(fd : usize, val : usize) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.set_fd_flags(task_id, fd, val))
}

/// 给当前任务 fd 区间内所有已打开 fd 设置/清除 `FD_CLOEXEC`；未打开 fd 忽略。
#[inline]
pub fn set_fd_range_cloexec(first : usize, last : usize, cloexec : bool) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.set_fd_range_cloexec(task_id, first, last, cloexec))
}

/// 当前任务下 `fd` 是否为 `O_PATH` 句柄。
#[inline]
pub fn is_path_only_fd(fd : usize) -> VfsResult<bool> {
    with_current_task(|reg, task_id| reg.is_fd_path_only(task_id, fd))
}

/// 将 `fd` 标记为 `O_PATH` 句柄。
#[inline]
pub fn set_path_only_fd(fd : usize) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.set_fd_path_only(task_id, fd))
}

pub use impl_fd_session::file_lock::{
    flock_op, inode_key_from_metadata, posix_getlk, posix_setlk, release_flock_owner,
    release_process_inode_locks, Flock, F_RDLCK, F_UNLCK, F_WRLCK, InodeKey, LOCK_EX, LOCK_NB,
    LOCK_SH, LOCK_UN,
};

/// fork 时初始化子任务 fd 表（仅默认 stdio，spawn 路径）。
#[inline]
pub fn init_child_fd_table(child_id : task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.init_child_fd_table(child_id);
}

/// fork 时复制父任务 fd 表。
#[inline]
pub fn copy_fd_table_from_parent(child_id : task::TaskId, parent_id : task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.copy_fd_table_from_parent(child_id, parent_id);
}

/// thread clone 时共享父任务 fd 表。
#[inline]
pub fn share_fd_table_from_parent(child_id : task::TaskId, parent_id : task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.share_fd_table_from_parent(child_id, parent_id);
}

/// `execve` 前关闭带 `FD_CLOEXEC` 的 fd。
pub fn close_cloexec_fds_for_current_task() -> VfsResult<()> {
    let task_id = current_task_id()?;
    let handles = {
        let mut reg = registry().exclusive_access();
        reg.take_cloexec_fds_for_task(task_id)
    };
    for mut handle in handles {
        handle.close()?;
    }
    Ok(())
}

/// 任务退出后释放 fd 表。
#[inline]
pub fn drop_task_fd_table(task_id : task::TaskId) {
    let handles = {
        let mut reg = registry().exclusive_access();
        reg.drain_task_fd_table(task_id)
    };
    for mut handle in handles {
        if let Err(e) = handle.close() {
            log::warn!("[vfs-fd] drop_task_fd_table task_id={task_id} close failed: {e:?}");
        }
    }
}

/// bring-up：两任务 fd 表隔离、dup 与 fork 继承烟囱。
pub fn self_test() {
    let mut reg = registry().exclusive_access();
    let stdio_task : task::TaskId = 20;
    assert!(reg.close_fd_for_task(stdio_task, api_v0::VFS_STDIN_FD)
               .is_ok());
    assert!(reg.close_fd_for_task(stdio_task, api_v0::VFS_STDIN_FD)
               .is_err());
    assert!(reg.get_io_for_task(stdio_task, api_v0::VFS_STDIN_FD)
               .is_err());
    let reused_stdin = reg.alloc_fd_for_task(stdio_task, stdio_replacement_handle())
                          .expect("alloc stdio");
    assert_eq!(reused_stdin, api_v0::VFS_STDIN_FD);
    reg.drop_task_fd_table(stdio_task);

    let a : task::TaskId = 10;
    let b : task::TaskId = 11;
    let fd = reg.alloc_fd_for_task(a,
                                   Box::new(impl_fd_session::ConsoleOutHandle))
                  .expect("alloc fd");
    let fd_b = reg.alloc_fd_for_task(b,
                                     Box::new(impl_fd_session::ConsoleOutHandle))
                  .expect("alloc fd");
    assert_eq!(fd, fd_b);
    assert!(reg.get_io_for_task(a, fd)
               .is_ok());
    assert!(reg.get_io_for_task(b, fd_b)
               .is_ok());
    let dup_fd = reg.dup_fd_for_task(a, fd, 0)
                    .expect("dup");
    assert_ne!(dup_fd, fd);
    assert!(reg.get_io_for_task(a, dup_fd)
               .is_ok());
    assert!(reg.close_fd_for_task(a, dup_fd)
               .is_ok());
    assert!(reg.close_fd_for_task(a, fd)
               .is_ok());
    assert!(reg.get_io_for_task(a, fd)
               .is_err());
    assert!(reg.get_io_for_task(b, fd_b)
               .is_ok());

    let parent_extra = reg.alloc_fd_for_task(a,
                                             Box::new(impl_fd_session::ConsoleOutHandle))
                          .expect("alloc fd");
    reg.copy_fd_table_from_parent(b, a);
    assert!(reg.get_io_for_task(b, parent_extra)
               .is_ok());
    assert!(reg.get_io_for_task(a, parent_extra)
               .is_ok());
    let c : task::TaskId = 12;
    reg.share_fd_table_from_parent(c, a);
    assert!(reg.get_io_for_task(c, parent_extra)
               .is_ok());
    reg.drop_task_fd_table(c);
    assert!(reg.get_io_for_task(a, parent_extra)
               .is_ok());

    let _ = reg.close_fd_for_task(b, fd_b);
    let _ = reg.close_fd_for_task(a, parent_extra);
    let _ = reg.close_fd_for_task(b, parent_extra);
    let fd_reuse = reg.alloc_fd_for_task(a,
                                         Box::new(impl_fd_session::ConsoleOutHandle))
                      .expect("alloc fd");
    assert_eq!(fd_reuse, fd);
    reg.drop_task_fd_table(a);
    reg.drop_task_fd_table(b);

    if impl_fd_session::poll_pipe_smoke() {
        log::info!("[poll] ppoll pipe ok");
    } else {
        log::warn!("[poll] ppoll pipe smoke failed");
    }
    if impl_fd_session::stream_pair_smoke() {
        log::info!("[socketpair] stream pair ok");
    } else {
        log::warn!("[socketpair] stream pair smoke failed");
    }
}

fn stdio_replacement_handle() -> Box<dyn VfsIoHandle> {
    #[cfg(feature = "bridge-fs-api")]
    {
        match fs::devfs::active_impl::lookup_character_device("/dev/null") {
            Ok(dev) => Box::new(impl_fd_session::CharDevHandle::from_devfs_path(dev, "/dev/null")),
            Err(err) => {
                log::warn!("[vfs][fd] /dev/null unavailable for stdio replacement: {:?}; \
                            fallback to zero handle",
                           err);
                Box::new(impl_fd_session::ZeroDeviceHandle)
            }
        }
    }
    #[cfg(not(feature = "bridge-fs-api"))]
    {
        Box::new(impl_fd_session::ZeroDeviceHandle)
    }
}
