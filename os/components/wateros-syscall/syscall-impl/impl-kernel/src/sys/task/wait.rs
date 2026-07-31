//! 等待子进程系统调用与退出清理辅助：`waitpid`、`waitid`、退出资源释放。
//! 本模块代码由AI完成
use alloc::vec::Vec;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use task::ProcessId;

use crate::sys::task::rlimit::RLIMIT_CORE;
use crate::sys::time::timer::{
    account_child_cpu, child_cpu_from_exited, ticks_to_timeval, write_child_rusage,
    write_zero_rusage, ChildCpuTicks,
};
use crate::user_copy::{
    atomic_compare_exchange_user_u32_in_aspace, atomic_load_user_u32_in_aspace,
    copy_from_user_struct, copy_to_user_struct,
};

/// 等待目标类型：任意子进程、指定进程组、或特定进程。
#[derive(Clone, Copy)]
enum WaitTarget {
    AnyChild,
    ProcessGroup(ProcessId),
    Specific(ProcessId),
}

const ORPHAN_PARENT_PID : usize = 1;
const WNOHANG : usize = 1;
const WUNTRACED : usize = 2;
const WCONTINUED : usize = 8;
const WEXITED : usize = 4;
const WNOWAIT : usize = 0x0100_0000;
const WAITPID_IGNORED_OPTIONS : usize = WUNTRACED | WCONTINUED;
const WAITID_EVENT_OPTIONS : usize = WEXITED | WUNTRACED | WCONTINUED;
const WAITID_ALLOWED_OPTIONS : usize = WNOHANG
    | WAITID_EVENT_OPTIONS
    | WNOWAIT
    | 0x8000_0000 // WCLONED
    | 0x4000_0000 // WALL
    | 0x2000_0000; // WTHREAD
const CLD_STOPPED : i32 = 5;
const CLD_CONTINUED : i32 = 6;
const WAITID_P_ALL : i32 = 0;
const WAITID_P_PID : i32 = 1;
const WAITID_P_PGID : i32 = 2;
const WAITID_P_PIDFD : i32 = 3;
const SIGCHLD : i32 = 17;
const CLD_EXITED : i32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserSigInfo {
    signo : i32,
    errno : i32,
    code : i32,
    payload : [u8; 116],
}

impl UserSigInfo {
    fn for_signal(sig : usize) -> Self {
        Self { signo : sig as i32,
               errno : 0,
               code : 0,
               payload : [0; 116] }
    }
}

/// 写 0 到 `clear_child_tid` 并 futex 唤醒 join 等待者；写失败仍唤醒。
pub(crate) fn wake_clear_child_tid_for_task(task_id : task::TaskId) -> usize {
    use core::sync::atomic::fence;

    let Some(clear_child_tid) = task::task_clear_child_tid(task_id) else {
        return 0;
    };
    let addr = clear_child_tid.user_addr();
    if addr == 0 {
        return 0;
    }
    let Some(task_snapshot) = task::process_task_snapshot(task_id) else {
        return 0;
    };
    let tid_raw = task_snapshot.tid
                               .raw();
    let Some(user_aspace) =
        task::process_snapshot(task_snapshot.pid).and_then(|process| process.address_space)
                                                 .map(|aspace| aspace.user_aspace_ptr())
    else {
        return 0;
    };
    let clear_result = (|| loop {
        let old = atomic_load_user_u32_in_aspace(user_aspace, addr)?;
        let observed = atomic_compare_exchange_user_u32_in_aspace(user_aspace, addr, old, 0)?;
        if observed == old {
            return Ok::<(), ErrNo>(());
        }
    })();
    fence(core::sync::atomic::Ordering::SeqCst);
    let woken = super::super::futex::wake_user_addr(user_aspace, addr);
    log::trace!("[pthread-debug] clear_child_tid task_id={} tid={} addr={:#x} write_ok={} \
                 woken={}",
                task_id,
                tid_raw,
                addr,
                clear_result.is_ok(),
                woken,);
    if let Err(err) = clear_result {
        log::warn!("[exit] clear_child_tid write failed task_id={} tid={} addr={:#x}: {:?}",
                   task_id,
                   tid_raw,
                   addr,
                   err,);
    }
    woken
}

/// 信号终止进程的 wait(2) 编码：负值表示被信号杀死，低 7 位为信号号，bit7 为 core dump。
pub(crate) fn signal_terminate_exit_code(signal : usize, task_id : usize) -> isize {
    let mut status = (signal & 0x7F) as isize;
    if let Some(snapshot) = task::process_task_snapshot(task_id) {
        if task::process_resource_limit(snapshot.pid, RLIMIT_CORE).map(|limit| limit.cur > 0)
                                                                  .unwrap_or(false)
        {
            status |= 0x80;
        }
    }
    -status
}

fn write_exit_code(exit_code_ptr : usize, exit_code : isize) -> Result<(), ErrNo> {
    if exit_code_ptr == 0 {
        return Ok(());
    }
    let wait_status = if exit_code < 0 {
        (-exit_code) as i32
    } else {
        ((exit_code as i32) & 0xFF) << 8
    };
    copy_to_user_struct(exit_code_ptr, &wait_status)
}

fn drop_exited_task_resources(exited : &task::ExitedTask) {
    let aspace = exited.trap_frame
                       .as_ref()
                       .map(|frame| frame.user_aspace_ptr())
                       .unwrap_or(0);
    drop_task_runtime_resources_with_aspace(exited.id, aspace);
}

pub(crate) fn drop_task_runtime_resources(task_id : task::TaskId) {
    let aspace = if task::current_task_id() == Some(task_id) {
        task::current_task_user_aspace_ptr()
    } else {
        0
    };
    drop_task_runtime_resources_with_aspace(task_id, aspace);
}

fn drop_task_runtime_resources_with_aspace(task_id : task::TaskId, aspace : usize) {
    ipc::futex::cancel_task_wait(task_id);
    super::super::shm::drop_task_attachments(task_id, aspace);
    vfs::cwd::drop_task_cwd(task_id);
    vfs::mount_ns::drop_task_mount_ns(task_id);
    vfs::fd::drop_task_fd_table(task_id);
    crate::epoll_fd::drop_task(task_id);
    crate::unix_sock::drop_task(task_id);
    cred::drop_task_cred(task_id);
}

pub(crate) fn reap_exited_member_threads_runtime_resources(pid : task::ProcessId) {
    let aspace = task::process_snapshot(pid).and_then(|process| process.address_space)
                                            .map(|address_space| address_space.user_aspace_ptr())
                                            .unwrap_or(0);
    let reaped = task::reap_exited_member_threads(pid);
    crate::sys::misc::bringup_stats::record_reap_member_threads(reaped.len());
    for exited in reaped {
        drop_reaped_task_runtime_resources(exited.id, aspace);
    }
}

pub(crate) fn drop_reaped_task_runtime_resources(task_id : task::TaskId, aspace : usize) {
    crate::sys::ipc::robust::drop_robust_state(task_id);
    crate::sys::ipc::signal::drop_thread_state(task_id);
    drop_task_runtime_resources_with_aspace(task_id, aspace);
}

// ── 子进程等待辅助 ─────────────────────────────────────────────

fn user_siginfo_exited(pid : task::ProcessId, exit_code : isize) -> UserSigInfo {
    let status = if exit_code < 0 {
        (-exit_code) as i32
    } else {
        ((exit_code as i32) & 0xFF) << 8
    };
    #[repr(C)]
    struct SigchldFields {
        pid : i32,
        uid : u32,
        status : i32,
        utime : isize,
        stime : isize,
    }
    let fields = SigchldFields { pid : pid.raw() as i32,
                                 uid : 0,
                                 status,
                                 utime : 0,
                                 stime : 0 };
    let mut info = UserSigInfo::for_signal(SIGCHLD as usize);
    info.code = CLD_EXITED;
    unsafe {
        core::ptr::copy_nonoverlapping(&fields as *const SigchldFields as *const u8,
                                       info.payload
                                           .as_mut_ptr(),
                                       core::mem::size_of::<SigchldFields>());
    }
    info
}

fn user_siginfo_stopped(pid : task::ProcessId, signo : u8) -> UserSigInfo {
    #[repr(C)]
    struct SigchldFields {
        pid : i32,
        uid : u32,
        status : i32,
        utime : isize,
        stime : isize,
    }
    let status = (signo as i32) << 8 | 0x7F;
    let fields = SigchldFields { pid : pid.raw() as i32,
                                 uid : 0,
                                 status,
                                 utime : 0,
                                 stime : 0 };
    let mut info = UserSigInfo::for_signal(SIGCHLD as usize);
    info.code = CLD_STOPPED;
    unsafe {
        core::ptr::copy_nonoverlapping(&fields as *const SigchldFields as *const u8,
                                       info.payload
                                           .as_mut_ptr(),
                                       core::mem::size_of::<SigchldFields>());
    }
    info
}

fn user_siginfo_continued(pid : task::ProcessId) -> UserSigInfo {
    #[repr(C)]
    struct SigchldFields {
        pid : i32,
        uid : u32,
        status : i32,
        utime : isize,
        stime : isize,
    }
    let fields = SigchldFields { pid : pid.raw() as i32,
                                 uid : 0,
                                 status : 0xFFFF,
                                 utime : 0,
                                 stime : 0 };
    let mut info = UserSigInfo::for_signal(SIGCHLD as usize);
    info.code = CLD_CONTINUED;
    unsafe {
        core::ptr::copy_nonoverlapping(&fields as *const SigchldFields as *const u8,
                                       info.payload
                                           .as_mut_ptr(),
                                       core::mem::size_of::<SigchldFields>());
    }
    info
}

fn finish_wait_process_result(parent_pid : task::ProcessId,
                              pid : task::ProcessId,
                              exited_tasks : Vec<task::ExitedTask>,
                              exit_code_ptr : usize,
                              rusage_ptr : usize)
                              -> UserRet {
    let Some(status_task) = exited_tasks.first() else {
        return UserRet::from_error(ErrNo::ECHILD);
    };
    match write_exit_code(exit_code_ptr, status_task.exit_code) {
        Ok(()) => {}
        Err(e) => return UserRet::from_error(e),
    }
    let child_cpu = child_cpu_from_exited(&exited_tasks);
    if let Err(e) = write_child_rusage(rusage_ptr, child_cpu) {
        return UserRet::from_error(e);
    }
    account_child_cpu(parent_pid, child_cpu);
    for exited in &exited_tasks {
        drop_exited_task_resources(exited);
    }
    UserRet::from_success(pid.raw())
}

fn finish_waitid_process_result(parent_pid : task::ProcessId,
                                pid : task::ProcessId,
                                exited_tasks : Vec<task::ExitedTask>,
                                siginfo_ptr : usize,
                                rusage_ptr : usize)
                                -> UserRet {
    let Some(status_task) = exited_tasks.first() else {
        return UserRet::from_error(ErrNo::ECHILD);
    };
    if siginfo_ptr != 0 {
        let info = user_siginfo_exited(pid, status_task.exit_code);
        if let Err(e) = copy_to_user_struct(siginfo_ptr, &info) {
            return UserRet::from_error(e);
        }
    }
    let child_cpu = child_cpu_from_exited(&exited_tasks);
    if let Err(e) = write_child_rusage(rusage_ptr, child_cpu) {
        return UserRet::from_error(e);
    }
    account_child_cpu(parent_pid, child_cpu);
    for exited in &exited_tasks {
        drop_exited_task_resources(exited);
    }
    UserRet::from_success(0)
}

fn finish_waitid_stopped_result(pid : task::ProcessId,
                                signo : u8,
                                siginfo_ptr : usize,
                                rusage_ptr : usize,
                                nowait : bool)
                                -> UserRet {
    if siginfo_ptr != 0 {
        let info = user_siginfo_stopped(pid, signo);
        if let Err(e) = copy_to_user_struct(siginfo_ptr, &info) {
            return UserRet::from_error(e);
        }
    }
    if let Err(e) = write_zero_rusage(rusage_ptr) {
        return UserRet::from_error(e);
    }
    task::consume_stop_wait(pid, nowait);
    UserRet::from_success(0)
}

fn finish_waitid_continued_result(pid : task::ProcessId,
                                  siginfo_ptr : usize,
                                  rusage_ptr : usize,
                                  nowait : bool)
                                  -> UserRet {
    if siginfo_ptr != 0 {
        let info = user_siginfo_continued(pid);
        if let Err(e) = copy_to_user_struct(siginfo_ptr, &info) {
            return UserRet::from_error(e);
        }
    }
    if let Err(e) = write_zero_rusage(rusage_ptr) {
        return UserRet::from_error(e);
    }
    task::consume_continued_wait(pid, nowait);
    UserRet::from_success(0)
}

fn wait_target_from_pid(pid : isize, caller_pgid : task::ProcessId) -> Result<WaitTarget, ErrNo> {
    if pid == -1 {
        return Ok(WaitTarget::AnyChild);
    }
    if pid == 0 {
        return Ok(WaitTarget::ProcessGroup(caller_pgid));
    }
    if pid < -1 {
        let pgid = task::ProcessId::from_raw((-pid) as usize);
        return Ok(WaitTarget::ProcessGroup(pgid));
    }
    Ok(WaitTarget::Specific(task::ProcessId::from_raw(pid as usize)))
}

fn find_exited_child_for_wait(parent_pid : task::ProcessId,
                              target : WaitTarget)
                              -> Option<task::ProcessSnapshot> {
    match target {
        WaitTarget::AnyChild => task::find_exited_child_process(parent_pid),
        WaitTarget::ProcessGroup(pgid) => task::find_exited_child_process_in_pgid(parent_pid, pgid),
        WaitTarget::Specific(child_pid) => {
            let child = task::process_snapshot(child_pid)?;
            if child.parent_pid != Some(parent_pid) {
                return None;
            }
            if !matches!(child.state,
                         task::ProcessState::Exited(_))
            {
                return None;
            }
            Some(child)
        }
    }
}

fn find_stopped_child_for_wait(parent_pid : task::ProcessId,
                               target : WaitTarget)
                               -> Option<task::ProcessSnapshot> {
    match target {
        WaitTarget::AnyChild => task::find_stopped_child_process(parent_pid),
        WaitTarget::ProcessGroup(pgid) => {
            task::find_stopped_child_process_in_pgid(parent_pid, pgid)
        }
        WaitTarget::Specific(child_pid) => {
            task::stopped_child_ready_for_wait(parent_pid, child_pid)
        }
    }
}

fn find_continued_child_for_wait(parent_pid : task::ProcessId,
                                 target : WaitTarget)
                                 -> Option<task::ProcessSnapshot> {
    match target {
        WaitTarget::AnyChild => task::find_continued_child_process(parent_pid),
        WaitTarget::ProcessGroup(pgid) => {
            task::find_continued_child_process_in_pgid(parent_pid, pgid)
        }
        WaitTarget::Specific(child_pid) => {
            task::continued_child_ready_for_wait(parent_pid, child_pid)
        }
    }
}

fn has_pending_wait_event(parent_pid : task::ProcessId,
                          target : WaitTarget,
                          want_exited : bool,
                          want_stopped : bool,
                          want_continued : bool)
                          -> bool {
    if want_exited && find_exited_child_for_wait(parent_pid, target).is_some() {
        return true;
    }
    if want_stopped && find_stopped_child_for_wait(parent_pid, target).is_some() {
        return true;
    }
    if want_continued && find_continued_child_for_wait(parent_pid, target).is_some() {
        return true;
    }
    false
}

fn has_waitable_child(parent_pid : task::ProcessId, target : WaitTarget) -> bool {
    match target {
        WaitTarget::AnyChild => task::has_child_process(parent_pid),
        WaitTarget::ProcessGroup(pgid) => task::has_child_process_in_pgid(parent_pid, pgid),
        WaitTarget::Specific(child_pid) => task::process_snapshot(child_pid).is_some_and(|child| {
                                                                                child.parent_pid ==
                                                                                Some(parent_pid)
                                                                            }),
    }
}

fn wait_for_child_exit(parent_pid : task::ProcessId,
                       target : WaitTarget,
                       exit_code_ptr : usize,
                       rusage_ptr : usize,
                       nohang : bool)
                       -> UserRet {
    loop {
        if let Some(child) = find_exited_child_for_wait(parent_pid, target) {
            let Some(exited) = task::reap_exited_process(child.pid) else {
                if task::process_snapshot(child.pid).is_some() {
                    task::yield_now();
                    continue;
                }
                return UserRet::from_error(ErrNo::ECHILD);
            };
            return finish_wait_process_result(parent_pid,
                                              child.pid,
                                              exited,
                                              exit_code_ptr,
                                              rusage_ptr);
        }
        if !has_waitable_child(parent_pid, target) {
            return UserRet::from_error(ErrNo::ECHILD);
        }
        if nohang {
            return UserRet::from_success(0);
        }
        if waitpid_wait_for_child(parent_pid, target) == task::TaskWaitResult::Interrupted {
            // SIGCHLD may become pending just before the child-exit wait queue
            // wakes us. Prefer the now-observable child result over EINTR;
            // otherwise callers that do not retry EINTR leave one zombie and
            // kernel stack behind per fork.
            if find_exited_child_for_wait(parent_pid, target).is_some() {
                continue;
            }
            return UserRet::from_error(ErrNo::EINTR);
        }
    }
}

fn waitid_for_child(parent_pid : task::ProcessId,
                    target : WaitTarget,
                    siginfo_ptr : usize,
                    rusage_ptr : usize,
                    nohang : bool,
                    nowait : bool,
                    want_exited : bool,
                    want_stopped : bool,
                    want_continued : bool)
                    -> UserRet {
    loop {
        if want_exited {
            if let Some(child) = find_exited_child_for_wait(parent_pid, target) {
                let Some(exited) = task::reap_exited_process(child.pid) else {
                    if task::process_snapshot(child.pid).is_some() {
                        task::yield_now();
                        continue;
                    }
                    return UserRet::from_error(ErrNo::ECHILD);
                };
                return finish_waitid_process_result(parent_pid,
                                                    child.pid,
                                                    exited,
                                                    siginfo_ptr,
                                                    rusage_ptr);
            }
        }
        if want_stopped {
            if let Some(child) = find_stopped_child_for_wait(parent_pid, target) {
                let signo = match child.state {
                    task::ProcessState::Stopped { signo } => signo,
                    _ => ipc::signal::SIGSTOP as u8,
                };
                return finish_waitid_stopped_result(child.pid,
                                                    signo,
                                                    siginfo_ptr,
                                                    rusage_ptr,
                                                    nowait);
            }
        }
        if want_continued {
            if let Some(child) = find_continued_child_for_wait(parent_pid, target) {
                return finish_waitid_continued_result(child.pid,
                                                      siginfo_ptr,
                                                      rusage_ptr,
                                                      nowait);
            }
        }
        if !has_waitable_child(parent_pid, target) {
            return UserRet::from_error(ErrNo::ECHILD);
        }
        if nohang {
            return UserRet::from_success(0);
        }
        if wait_for_child_event(parent_pid,
                                target,
                                want_exited,
                                want_stopped,
                                want_continued) ==
           task::TaskWaitResult::Interrupted
        {
            if has_pending_wait_event(parent_pid,
                                      target,
                                      want_exited,
                                      want_stopped,
                                      want_continued)
            {
                continue;
            }
            return UserRet::from_error(ErrNo::EINTR);
        }
    }
}

fn validate_wait_options(options : usize, allow_exited : bool) -> Result<(), ErrNo> {
    let allowed = if allow_exited {
        WAITID_ALLOWED_OPTIONS
    } else {
        WNOHANG | WAITPID_IGNORED_OPTIONS
    };
    if options & !allowed != 0 {
        return Err(ErrNo::EINVAL);
    }
    if allow_exited && (options & WAITID_EVENT_OPTIONS) == 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

/// `waitpid`/`wait4`：维护父子关系并阻塞等待子任务退出。
pub(crate) fn sys_waitpid(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let exit_code_ptr = args.arg(1);
    let options = args.arg(2);
    let rusage_ptr = args.arg(3);
    let nohang = (options & WNOHANG) != 0;
    if validate_wait_options(options, false).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let current_process = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let target = match wait_target_from_pid(pid, current_process.pgid) {
        Ok(target) => target,
        Err(error) => return UserRet::from_error(error),
    };
    if let WaitTarget::Specific(child_pid) = target {
        match task::process_snapshot(child_pid) {
            Some(snapshot) if snapshot.parent_pid == Some(current_process.pid) => {}
            Some(_) => return UserRet::from_error(ErrNo::ECHILD),
            None => return UserRet::from_error(ErrNo::ECHILD),
        }
    }
    wait_for_child_exit(current_process.pid,
                        target,
                        exit_code_ptr,
                        rusage_ptr,
                        nohang)
}

/// `waitid(idtype, id, infop, options, rusage)`。
pub(crate) fn sys_waitid(args : SyscallArgs) -> UserRet {
    let idtype = args.arg(0) as i32;
    let id = args.arg(1) as i32;
    let siginfo_ptr = args.arg(2);
    let options = args.arg(3);
    let rusage_ptr = args.arg(4);
    let nohang = (options & WNOHANG) != 0;
    if validate_wait_options(options, true).is_err() {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if siginfo_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let current_process = match task::current_process_snapshot() {
        Some(process) => process,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let target = match idtype {
        WAITID_P_ALL => WaitTarget::AnyChild,
        WAITID_P_PID => {
            if id <= 0 {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            WaitTarget::Specific(task::ProcessId::from_raw(id as usize))
        }
        WAITID_P_PGID => {
            if id < 0 {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            let pgid = if id == 0 {
                current_process.pgid
            } else {
                task::ProcessId::from_raw(id as usize)
            };
            WaitTarget::ProcessGroup(pgid)
        }
        WAITID_P_PIDFD => return UserRet::from_error(ErrNo::EINVAL),
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };
    if let WaitTarget::Specific(child_pid) = target {
        match task::process_snapshot(child_pid) {
            Some(snapshot) if snapshot.parent_pid == Some(current_process.pid) => {}
            Some(_) => return UserRet::from_error(ErrNo::ECHILD),
            None => return UserRet::from_error(ErrNo::ECHILD),
        }
    }
    waitid_for_child(current_process.pid,
                     target,
                     siginfo_ptr,
                     rusage_ptr,
                     nohang,
                     (options & WNOWAIT) != 0,
                     (options & WEXITED) != 0,
                     (options & WUNTRACED) != 0,
                     (options & WCONTINUED) != 0)
}

/// 利用 ChildExit wait queue 事件驱动等待，替代原有的轮询 sleep。
fn wait_for_child_event(parent_pid : task::ProcessId,
                        target : WaitTarget,
                        want_exited : bool,
                        want_stopped : bool,
                        want_continued : bool)
                        -> task::TaskWaitResult {
    let Some(leader) = task::leader_task_for_process(parent_pid) else {
        return task::TaskWaitResult::Woken;
    };
    let wait_target = task::TaskWaitTarget::ChildExit(leader);
    task::wait_on_while(wait_target, || {
        has_waitable_child(parent_pid, target) &&
        !has_pending_wait_event(parent_pid,
                                target,
                                want_exited,
                                want_stopped,
                                want_continued)
    })
}

/// `waitpid`/`wait4` 使用的子进程退出等待。
fn waitpid_wait_for_child(parent_pid : task::ProcessId,
                          target : WaitTarget)
                          -> task::TaskWaitResult {
    wait_for_child_event(parent_pid, target, true, false, false)
}
