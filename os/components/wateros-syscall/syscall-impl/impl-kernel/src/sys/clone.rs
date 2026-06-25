//! `clone`/`fork` 系统调用实现。
//!
//! fork 时会为子进程创建**独立地址空间**（通过 `mm::kernel_mm::fork_user_aspace`），
//! 复制父进程 trap 帧（a0 置 0 作为子进程返回值），继承 cwd 与 fd 表（经 VFS duplicate）。
//!
//! clone（`child_stack ≠ 0`）时子进程使用调用者提供的独立栈。
//! fork（`child_stack == 0`）时子进程 SP 由 [`task`] 层 `fork_from` 按父栈区间设置。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_from_user, copy_to_user_struct};

const CLONE3_ARGS_SIZE_V0 : usize = 64;
const CLONE3_ARGS_SIZE_CURRENT : usize = 88;
const CLONE3_EXIT_SIGNAL_MASK : usize = 0xFF;
const CLONE_PIDFD : usize = 0x0000_1000;
const CLONE_INTO_CGROUP : usize = 0x0000_0002_0000_0000;
/// Linux `CSIGNAL`：fork 路径仅允许低 8 位退出信号号。
const CLONE_CSIGNAL_MASK : usize = 0xFF;

struct CloneRequest {
    clone_flags : task::CloneFlags,
    child_stack : usize,
    parent_tid : usize,
    tls : usize,
    child_tid : usize,
}

struct CloneSetupGuard {
    state : platform::arch::interrupt::ArchInterruptState,
}

impl CloneSetupGuard {
    fn new() -> Result<Self, ErrNo> {
        let state = platform::arch::interrupt::read_global_interrupt_state()
            .map_err(|_| ErrNo::EIO)?;
        platform::arch::interrupt::disable_global_interrupt().map_err(|_| ErrNo::EIO)?;
        Ok(Self { state })
    }
}

impl Drop for CloneSetupGuard {
    fn drop(&mut self) {
        let _ = platform::arch::interrupt::restore_global_interrupt_state(self.state);
    }
}

/// clone/fork 系统调用入口。
///
/// 参数（Linux legacy `clone` raw syscall ABI）：
/// - `arg0`: flags
/// - `arg1`: child_stack（0 表示复用父任务栈指针）
/// - `arg2`: parent_tid
/// - RISC-V: `arg3`: tls, `arg4`: child_tid
/// - LoongArch: `arg3`: child_tid, `arg4`: tls
pub(crate) fn sys_clone(args : SyscallArgs) -> UserRet { do_clone(args) }

/// clone3 系统调用入口。
///
/// Linux `struct clone_args` 通过 `(uaddr, size)` 传入；当前实现读取内核认识的
/// 88 字节版本，并将可支持字段转换为已有 `clone` 入口。
pub(crate) fn sys_clone3(args : SyscallArgs) -> UserRet {
    let clone_args = match Clone3Args::read_from_user(args.arg(0), args.arg(1)) {
        Ok(args) => args,
        Err(error) => return UserRet::from_error(error),
    };
    if clone_args.flags & CLONE3_EXIT_SIGNAL_MASK != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if clone_args.exit_signal & !CLONE3_EXIT_SIGNAL_MASK != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if clone_args.flags & CLONE_PIDFD != 0 || clone_args.pidfd != 0 {
        return UserRet::from_error(ErrNo::ENOSYS);
    }
    if clone_args.set_tid != 0 || clone_args.set_tid_size != 0 {
        return UserRet::from_error(ErrNo::ENOSYS);
    }
    if clone_args.flags & CLONE_INTO_CGROUP != 0 || clone_args.cgroup != 0 {
        return UserRet::from_error(ErrNo::ENOSYS);
    }

    let child_stack = match clone3_child_stack(clone_args.stack, clone_args.stack_size) {
        Some(sp) => sp,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    do_clone_request(CloneRequest {
        clone_flags : task::CloneFlags::from_bits(clone_args.flags | clone_args.exit_signal),
        child_stack,
        parent_tid : clone_args.parent_tid,
        tls : clone_args.tls,
        child_tid : clone_args.child_tid,
    })
}

#[inline(never)]
fn do_clone(args : SyscallArgs) -> UserRet {
    do_clone_request(decode_legacy_clone_args(args))
}

#[cfg(target_arch = "loongarch64")]
fn decode_legacy_clone_args(args : SyscallArgs) -> CloneRequest {
    CloneRequest { clone_flags : task::CloneFlags::from_bits(args.arg(0)),
                   child_stack : args.arg(1),
                   parent_tid : args.arg(2),
                   tls : args.arg(4),
                   child_tid : args.arg(3) }
}

#[cfg(not(target_arch = "loongarch64"))]
fn decode_legacy_clone_args(args : SyscallArgs) -> CloneRequest {
    CloneRequest { clone_flags : task::CloneFlags::from_bits(args.arg(0)),
                   child_stack : args.arg(1),
                   parent_tid : args.arg(2),
                   tls : args.arg(3),
                   child_tid : args.arg(4) }
}

fn do_clone_request(request : CloneRequest) -> UserRet {
    let parent_signal = match super::signal::ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let CloneRequest { clone_flags,
                       child_stack,
                       parent_tid,
                       tls,
                       child_tid } = request;

    if clone_flags.contains(task::CloneFlags::CLONE_THREAD) &&
       !clone_flags.contains(task::CloneFlags::CLONE_VM)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_SIGHAND) &&
       !clone_flags.contains(task::CloneFlags::CLONE_VM)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_THREAD) &&
       !clone_flags.contains(task::CloneFlags::CLONE_SIGHAND)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let is_thread = clone_flags.contains(task::CloneFlags::CLONE_VM) &&
                    clone_flags.contains(task::CloneFlags::CLONE_THREAD);
    if is_thread {
        if let Some(process_task) = task::current_process_task_snapshot() {
            super::task::reap_exited_member_threads_runtime_resources(process_task.pid);
        }
        return do_clone_thread(clone_flags,
                               child_stack,
                               parent_tid,
                               tls,
                               child_tid);
    }

    if let Some(process_task) = task::current_process_task_snapshot() {
        if process_task.role != task::ProcessTaskRole::Leader {
            log::warn!(
                "[syscall] clone(nr=220) fork from non-leader tid={} pid={}",
                process_task.tid.raw(),
                process_task.pid.raw(),
            );
            return UserRet::from_error(ErrNo::EPERM);
        }
    }
    if let Err(errno) = validate_fork_clone_flags(clone_flags) {
        return UserRet::from_error(errno);
    }

    let parent_aspace = task::current_task_user_aspace_ptr();
    let (new_aspace_ptr, new_satp) = match mm::kernel_mm::fork_user_aspace(parent_aspace) {
        Ok(p) => p,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };

    let _setup_guard = match CloneSetupGuard::new() {
        Ok(guard) => guard,
        Err(error) => {
            mm::kernel_mm::drop_user_aspace(new_aspace_ptr);
            return UserRet::from_error(error);
        }
    };
    let child_id = match task::fork_current(child_stack, new_aspace_ptr, new_satp) {
        Some(id) => id,
        None => {
            mm::kernel_mm::drop_user_aspace(new_aspace_ptr);
            return UserRet::from_error(ErrNo::EAGAIN);
        }
    };
    let child_snapshot = match task::process_task_snapshot(child_id) {
        Some(snapshot) => snapshot,
        None => {
            if let Some(child_pid) = task::abort_fork_child(child_id) {
                super::signal::abort_fork_signal(child_pid.raw(), child_id);
            }
            return UserRet::from_error(ErrNo::ESRCH);
        }
    };
    let child_pid = child_snapshot.pid
                                  .raw();
    if super::signal::on_fork(parent_signal.task_id,
                              child_pid,
                              child_id,
                              child_snapshot.tid
                                            .raw()).is_err()
    {
        if let Some(rolled_pid) = task::abort_fork_child(child_id) {
            super::signal::abort_fork_signal(rolled_pid.raw(), child_id);
        }
        return UserRet::from_error(ErrNo::EAGAIN);
    }

    // 子任务继承父任务 cwd
    let parent_id = task::current_task_id().expect("current task must exist after fork");
    vfs::cwd::copy_cwd_from_parent(child_id, parent_id);

    vfs::fd::copy_fd_table_from_parent(child_id, parent_id);
    crate::socket_fd::copy_from_parent(child_id, parent_id);
    crate::unix_sock::copy_fds_from_parent(child_id, parent_id);

    cred::fork_cred(parent_id, child_id);
    if let Err(error) = super::shm::fork_task_attachments(parent_id, child_id, new_aspace_ptr) {
        log::warn!("[sys_clone] failed to inherit shm attachments: {:?}",
                   error);
    }

    UserRet::from_success(child_pid)
}

fn do_clone_thread(clone_flags : task::CloneFlags,
                   child_stack : usize,
                   parent_tid : usize,
                   tls : usize,
                   child_tid : usize)
                   -> UserRet {
    let clear_child_tid = if clone_flags.contains(task::CloneFlags::CLONE_CHILD_CLEARTID) {
        Some(task::TaskClearTid::new(child_tid))
    } else {
        None
    };
    let _setup_guard = match CloneSetupGuard::new() {
        Ok(guard) => guard,
        Err(error) => return UserRet::from_error(error),
    };
    let child_id = match task::clone_current_thread(child_stack,
                                                    tls,
                                                    clone_flags,
                                                    clear_child_tid)
    {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::EAGAIN),
    };
    let child_tid_raw = match task::process_task_snapshot(child_id) {
        Some(snapshot) => snapshot.tid.raw(),
        None => {
            task::abort_clone_thread(child_id);
            super::signal::abort_clone_thread_signal(child_id);
            return UserRet::from_error(ErrNo::ESRCH);
        }
    };
    let parent_id = task::current_task_id().expect("current task must exist after clone");
    if super::signal::on_clone_thread(parent_id, child_id, child_tid_raw).is_err() {
        task::abort_clone_thread(child_id);
        super::signal::abort_clone_thread_signal(child_id);
        return UserRet::from_error(ErrNo::EAGAIN);
    }
    let child_tid_value = child_tid_raw as u32;

    if clone_flags.contains(task::CloneFlags::CLONE_PARENT_SETTID) &&
       parent_tid != 0 &&
       copy_to_user_struct(parent_tid, &child_tid_value).is_err()
    {
        task::abort_clone_thread(child_id);
        super::signal::abort_clone_thread_signal(child_id);
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_CHILD_SETTID) &&
       child_tid != 0 &&
       copy_to_user_struct(child_tid, &child_tid_value).is_err()
    {
        task::abort_clone_thread(child_id);
        super::signal::abort_clone_thread_signal(child_id);
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if clone_flags.contains(task::CloneFlags::CLONE_FS) {
        vfs::cwd::share_cwd_from_parent(child_id, parent_id);
    } else {
        vfs::cwd::copy_cwd_from_parent(child_id, parent_id);
    }
    if clone_flags.contains(task::CloneFlags::CLONE_FILES) {
        vfs::fd::share_fd_table_from_parent(child_id, parent_id);
        crate::socket_fd::share_from_parent(child_id, parent_id);
    } else {
        vfs::fd::copy_fd_table_from_parent(child_id, parent_id);
        crate::socket_fd::copy_from_parent(child_id, parent_id);
    }
    crate::unix_sock::copy_fds_from_parent(child_id, parent_id);
    cred::share_cred(parent_id, child_id);

    super::bringup_stats::record_clone_thread();
    UserRet::from_success(child_tid_raw)
}

#[derive(Clone, Copy, Default)]
struct Clone3Args {
    flags : usize,
    pidfd : usize,
    child_tid : usize,
    parent_tid : usize,
    exit_signal : usize,
    stack : usize,
    stack_size : usize,
    tls : usize,
    set_tid : usize,
    set_tid_size : usize,
    cgroup : usize,
}

impl Clone3Args {
    fn read_from_user(ptr : usize, size : usize) -> Result<Self, ErrNo> {
        if ptr == 0 {
            return Err(ErrNo::EFAULT);
        }
        if size < CLONE3_ARGS_SIZE_V0 {
            return Err(ErrNo::EINVAL);
        }
        let mut raw = [0u8; CLONE3_ARGS_SIZE_CURRENT];
        let copy_len = size.min(CLONE3_ARGS_SIZE_CURRENT);
        let copied = copy_from_user(&mut raw[..copy_len], ptr)?;
        if copied != copy_len {
            return Err(ErrNo::EFAULT);
        }
        Ok(Self { flags : clone3_arg_word(&raw, 0),
                  pidfd : clone3_arg_word(&raw, 8),
                  child_tid : clone3_arg_word(&raw, 16),
                  parent_tid : clone3_arg_word(&raw, 24),
                  exit_signal : clone3_arg_word(&raw, 32),
                  stack : clone3_arg_word(&raw, 40),
                  stack_size : clone3_arg_word(&raw, 48),
                  tls : clone3_arg_word(&raw, 56),
                  set_tid : clone3_arg_word(&raw, 64),
                  set_tid_size : clone3_arg_word(&raw, 72),
                  cgroup : clone3_arg_word(&raw, 80) })
    }
}

fn clone3_arg_word(raw : &[u8; CLONE3_ARGS_SIZE_CURRENT], offset : usize) -> usize {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&raw[offset..offset + 8]);
    u64::from_ne_bytes(bytes) as usize
}

fn clone3_child_stack(stack : usize, stack_size : usize) -> Option<usize> {
    if stack == 0 {
        return Some(0);
    }
    if stack_size == 0 {
        return Some(stack);
    }
    stack.checked_add(stack_size)
}

/// fork 路径（非 `CLONE_VM|CLONE_THREAD`）仅接受 `CSIGNAL` 低 8 位；其余 flag 拒绝。
fn validate_fork_clone_flags(clone_flags : task::CloneFlags) -> Result<(), ErrNo> {
    let bits = clone_flags.bits();
    let unsupported = bits & !CLONE_CSIGNAL_MASK;
    if unsupported != 0 {
        log::warn!(
            "[syscall] clone(nr=220) fork unsupported flags={:#x} (allowed CSIGNAL={:#x})",
            unsupported,
            bits & CLONE_CSIGNAL_MASK,
        );
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}
