//! per-task fd 会话：全局注册表与当前任务便捷访问。

#![cfg(feature = "impl-fd-session")]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use api_v0::{VfsError, VfsIoHandle, VfsResult};
use base::sync::MultiprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};
use impl_fd_session::PerTaskFdRegistry;

static mut FD_REGISTRY : MaybeUninit<MultiprocessorSafeCell<PerTaskFdRegistry>> =
    MaybeUninit::uninit();
static FD_REGISTRY_READY : AtomicUsize = AtomicUsize::new(0);

/// 全局文件描述符注册表的只读调试摘要。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FdRegistryStats {
    /// 关联 fd 表的任务数；共享 fd 表的线程会分别计数。
    pub task_bindings : usize,
    /// 实际独立 fd 表数。
    pub table_count : usize,
    /// 所有独立 fd 表中当前已打开的描述符槽位数（含 stdio）。
    pub open_fd_count : usize,
}

/// 全局 per-task fd 注册表（自旋锁保护）。
pub fn registry() -> &'static MultiprocessorSafeCell<PerTaskFdRegistry> {
    if FD_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            FD_REGISTRY.write(MultiprocessorSafeCell::new(PerTaskFdRegistry::new()));
        }
        FD_REGISTRY_READY.store(1, Ordering::Release);
    }
    unsafe { &*FD_REGISTRY.as_ptr() }
}

/// 当前任务 id；无运行任务时 [`VfsError::NoTask`]。
pub fn current_task_id() -> VfsResult<task::TaskId> {
    task::current_task_id().ok_or(VfsError::NoTask)
}

/// 在“本核不可抢占 + 跨核互斥”的临界区内访问 fd 注册表。
///
/// 闭包内只应执行短小的注册表操作；不要阻塞，也不要调用可能再次访问 fd 表的代码。
pub fn with_registry<R>(f : impl FnOnce(&mut PerTaskFdRegistry) -> R) -> R {
    impl_fd_session::with_interrupt_disabled(|| {
        let mut registry = registry().exclusive_access();
        f(&mut registry)
    })
}

/// 返回全局 fd 表摘要；仅用于 dashboard 等低频诊断路径。
pub fn registry_stats() -> FdRegistryStats {
    // 观测不应改变系统状态：尚未有任何 fd 操作时不懒初始化注册表。
    if FD_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        return FdRegistryStats::default();
    }
    with_registry(|registry| {
        let (task_bindings, table_count, open_fd_count) = registry.debug_counts();
        FdRegistryStats { task_bindings,
                          table_count,
                          open_fd_count }
    })
}

fn with_fd_registry<R>(f : impl FnOnce(&mut PerTaskFdRegistry) -> VfsResult<R>) -> VfsResult<R> {
    with_registry(f)
}

/// 在持有注册表锁的情况下执行 `f`（传入可变注册表与当前任务 id）。
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
    with_task_io(task_id, fd, f)
}

fn with_task_io<R>(task_id : task::TaskId,
                   fd : usize,
                   f : impl FnOnce(&mut (dyn VfsIoHandle + '_)) -> VfsResult<R>)
                   -> VfsResult<R> {
    let handle = with_fd_registry(|reg| reg.io_handle_for_task(task_id, fd))?;
    handle.with_io(f)
}

/// 为当前任务分配 fd。
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
pub fn close_fd(fd : usize) -> VfsResult<()> {
    let task_id = current_task_id()?;
    let handle = with_fd_registry(|reg| reg.take_fd_for_close(task_id, fd))?;
    handle.with_io(|io| {
              release_locks_for_current_process(io);
              Ok(())
          })?;
    handle.close()
}

/// 关闭当前任务 fd 区间内所有已打开 fd；未打开 fd 按 Linux `close_range` 语义忽略。
pub fn close_fd_range(first : usize, last : usize) -> VfsResult<Vec<usize>> {
    let task_id = current_task_id()?;
    let handles = with_fd_registry(|reg| reg.take_fd_range_for_close(task_id, first, last))?;
    let mut closed = Vec::new();
    for (fd, handle) in handles {
        handle.with_io(|io| {
                  release_locks_for_current_process(io);
                  Ok(())
              })?;
        handle.close()?;
        closed.push(fd);
    }
    Ok(closed)
}

/// 请求全部打开句柄写回脏数据。
pub fn flush_all_open_files() -> VfsResult<()> {
    let handles = with_registry(|registry| registry.all_open_handles());
    let mut first_error = None;
    for handle in handles {
        if let Err(err) = handle.with_io(|io| io.flush()) {
            first_error.get_or_insert(err);
        }
    }
    first_error.map_or(Ok(()), Err)
}

/// 当前任务下 `fd` 是否为 TTY 类字符设备。
pub fn current_fd_is_tty_char(fd : usize) -> VfsResult<bool> {
    with_current_io(fd, |handle| {
        Ok(handle.is_tty_char_device())
    })
}

/// 当前任务下 `fd` 是否为软件 RTC 字符设备。
pub fn current_fd_is_rtc(fd : usize) -> VfsResult<bool> {
    with_current_io(fd, |handle| {
        if handle.is_rtc_device() {
            return Ok(true);
        }
        Ok(handle.metadata()
                 .map(|m| m.mode == 0o20644)
                 .unwrap_or(false))
    })
}

/// `dup(oldfd)`：复制到 ≥ `minfd` 的最低可用 fd。
pub fn dup_fd(oldfd : usize, minfd : usize) -> VfsResult<usize> {
    let task_id = current_task_id()?;
    let dup_handle = with_fd_registry(|reg| reg.duplicate_handle_for_task(task_id, oldfd))?;
    with_fd_registry(|reg| reg.install_dup_fd_for_task(task_id, minfd, dup_handle))
}

/// `dup3(oldfd, newfd, cloexec)`。
pub fn dup3_fd(oldfd : usize, newfd : usize, cloexec : bool) -> VfsResult<usize> {
    let task_id = current_task_id()?;
    if oldfd == newfd {
        with_fd_registry(|reg| {
            reg.io_handle_for_task(task_id, oldfd)
               .map(|_| ())
        })?;
        if cloexec {
            set_fd_flags(newfd, 1)?;
        }
        return Ok(newfd);
    }
    let dup_handle = with_fd_registry(|reg| reg.duplicate_handle_for_task(task_id, oldfd))?;
    let (fd, displaced) = with_fd_registry(|registry| {
        registry.install_dup3_fd_for_task(task_id, newfd, cloexec, dup_handle)
    })?;
    if let Some(handle) = displaced {
        let _ = handle.with_io(|io| {
                          release_locks_for_current_process(io);
                          Ok(())
                      });
        // dup3 已经原子替换 fd；Linux 语义不向调用方报告被覆盖 fd 的 close 错误。
        let _ = handle.close();
    }
    Ok(fd)
}

/// `fcntl(F_GETFD)`。
pub fn get_fd_flags(fd : usize) -> VfsResult<usize> {
    with_current_task(|reg, task_id| reg.get_fd_flags(task_id, fd))
}

/// `fcntl(F_SETFD)`。
pub fn set_fd_flags(fd : usize, val : usize) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.set_fd_flags(task_id, fd, val))
}

/// 给当前任务 fd 区间内所有已打开 fd 设置/清除 `FD_CLOEXEC`；未打开 fd 忽略。
pub fn set_fd_range_cloexec(first : usize, last : usize, cloexec : bool) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.set_fd_range_cloexec(task_id, first, last, cloexec))
}

/// 当前任务下 `fd` 是否为 `O_PATH` 句柄。
pub fn is_path_only_fd(fd : usize) -> VfsResult<bool> {
    with_current_task(|reg, task_id| reg.is_fd_path_only(task_id, fd))
}

/// 将 `fd` 标记为 `O_PATH` 句柄。
pub fn set_path_only_fd(fd : usize) -> VfsResult<()> {
    with_current_task(|reg, task_id| reg.set_fd_path_only(task_id, fd))
}

pub use impl_fd_session::file_lock::{
    flock_op, inode_key_from_metadata, posix_getlk, posix_setlk, release_flock_owner,
    release_process_inode_locks, Flock, InodeKey, F_RDLCK, F_UNLCK, F_WRLCK, LOCK_EX, LOCK_NB,
    LOCK_SH, LOCK_UN,
};

/// fork 时初始化子任务 fd 表（仅默认 stdio，spawn 路径）。
pub fn init_child_fd_table(child_id : task::TaskId) {
    with_registry(|registry| registry.init_child_fd_table(child_id));
}

/// fork 时复制父任务 fd 表。
pub fn copy_fd_table_from_parent(child_id : task::TaskId, parent_id : task::TaskId) {
    with_registry(|registry| {
        let (parent_table, parent_flags) = registry.fd_table_copy_plan(parent_id);
        registry.install_fd_table_copy(child_id, parent_table, parent_flags);
    });
}

/// thread clone 时共享父任务 fd 表。
pub fn share_fd_table_from_parent(child_id : task::TaskId, parent_id : task::TaskId) {
    with_registry(|registry| registry.share_fd_table_from_parent(child_id, parent_id));
}

/// `execve` 前关闭带 `FD_CLOEXEC` 的 fd。
pub fn close_cloexec_fds_for_current_task() -> VfsResult<Vec<usize>> {
    let task_id = current_task_id()?;
    let handles = with_registry(|registry| registry.take_cloexec_fds_for_task(task_id));
    let mut closed = Vec::with_capacity(handles.len());
    for (fd, handle) in handles {
        handle.close()?;
        closed.push(fd);
    }
    Ok(closed)
}

/// 任务退出后释放 fd 表。
pub fn drop_task_fd_table(task_id : task::TaskId) {
    let handles = with_registry(|registry| registry.drain_task_fd_table(task_id));
    for handle in handles {
        if let Err(e) = handle.close() {
            log::warn!("[vfs-fd] drop_task_fd_table task_id={task_id} close failed: {e:?}");
        }
    }
}

/// bring-up：两任务 fd 表隔离、dup 与 fork 继承烟囱。
pub fn self_test() {
    with_registry(|reg| {
        let stdio_task : task::TaskId = 20;
        assert!(reg.close_fd_for_task(stdio_task, api_v0::VFS_STDIN_FD)
                   .is_ok());
        assert!(reg.close_fd_for_task(stdio_task, api_v0::VFS_STDIN_FD)
                   .is_err());
        assert!(reg.io_handle_for_task(stdio_task, api_v0::VFS_STDIN_FD)
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
        assert!(reg.io_handle_for_task(a, fd)
                   .is_ok());
        assert!(reg.io_handle_for_task(b, fd_b)
                   .is_ok());
        let dup_handle = reg.duplicate_handle_for_task(a, fd)
                            .expect("dup source");
        let dup_fd = reg.install_dup_fd_for_task(a, 0, dup_handle)
                        .expect("dup");
        assert_ne!(dup_fd, fd);
        reg.set_fd_flags(a,
                         fd,
                         usize::from(impl_fd_session::registry::FD_CLOEXEC))
           .expect("set source cloexec");
        assert_eq!(reg.get_fd_flags(a, fd),
                   Ok(usize::from(impl_fd_session::registry::FD_CLOEXEC)));
        assert_eq!(reg.get_fd_flags(a, dup_fd),
                   Ok(0),
                   "dup descriptor flags must be independent");
        assert!(reg.io_handle_for_task(a, dup_fd)
                   .is_ok());
        assert!(reg.close_fd_for_task(a, dup_fd)
                   .is_ok());
        assert!(reg.close_fd_for_task(a, fd)
                   .is_ok());
        assert!(reg.io_handle_for_task(a, fd)
                   .is_err());
        assert!(reg.io_handle_for_task(b, fd_b)
                   .is_ok());

        let parent_extra = reg.alloc_fd_for_task(a,
                                                 Box::new(impl_fd_session::ConsoleOutHandle))
                              .expect("alloc fd");
        reg.set_fd_flags(a,
                         parent_extra,
                         usize::from(impl_fd_session::registry::FD_CLOEXEC))
           .expect("set inherited cloexec");
        let (parent_table, parent_flags) = reg.fd_table_copy_plan(a);
        reg.install_fd_table_copy(b, parent_table, parent_flags);
        assert!(reg.io_handle_for_task(b, parent_extra)
                   .is_ok());
        assert!(reg.io_handle_for_task(a, parent_extra)
                   .is_ok());
        assert_eq!(reg.get_fd_flags(b, parent_extra),
                   Ok(usize::from(impl_fd_session::registry::FD_CLOEXEC)));
        reg.set_fd_flags(b, parent_extra, 0)
           .expect("clear child cloexec");
        assert_eq!(reg.get_fd_flags(a, parent_extra),
                   Ok(usize::from(impl_fd_session::registry::FD_CLOEXEC)),
                   "fork descriptor flags must use separate tables");
        let c : task::TaskId = 12;
        reg.share_fd_table_from_parent(c, a);
        assert!(reg.io_handle_for_task(c, parent_extra)
                   .is_ok());
        reg.drop_task_fd_table(c);
        assert!(reg.io_handle_for_task(a, parent_extra)
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

        // An in-flight stdio operation must keep the old table alive while the
        // task exits. Reusing the task id must receive a fresh stdio table.
        let lease_task : task::TaskId = 30;
        let old_handle = reg.io_handle_for_task(lease_task, api_v0::VFS_STDIN_FD)
                            .expect("stdio lease");
        reg.drop_task_fd_table(lease_task);
        let new_fd = reg.alloc_fd_for_task(lease_task,
                                           Box::new(impl_fd_session::ConsoleOutHandle))
                        .expect("reuse task id");
        assert_eq!(new_fd, api_v0::VFS_FIRST_DYNAMIC_FD);
        old_handle.close()
                  .expect("close detached stdio");
        assert!(reg.io_handle_for_task(lease_task, api_v0::VFS_STDIN_FD)
                   .is_ok());
        reg.drop_task_fd_table(lease_task);

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
    });
}

fn stdio_replacement_handle() -> Box<dyn VfsIoHandle> {
    #[cfg(feature = "bridge-fs-api")]
    {
        match fs::devfs::active_impl::lookup_character_device("/dev/null") {
            Ok(dev) => {
                Box::new(impl_fd_session::CharDevHandle::from_devfs_path(dev, "/dev/null", 2))
            }
            Err(err) => {
                log::warn!("[vfs][fd] /dev/null unavailable for stdio replacement: {:?}; \
                            fallback to zero handle",
                           err);
                Box::new(impl_fd_session::ZeroDeviceHandle::new(2))
            }
        }
    }
    #[cfg(not(feature = "bridge-fs-api"))]
    {
        Box::new(impl_fd_session::ZeroDeviceHandle::new(2))
    }
}
