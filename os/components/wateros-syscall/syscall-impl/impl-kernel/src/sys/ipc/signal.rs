//! 进程/线程信号路由与 Linux 信号类 syscall 辅助逻辑。

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::{SignalDelivery, SignalDispatch, SignalError, SignalSet};
use platform::arch::trap::ActiveTrapFrame;
use task::{ProcessId, ThreadId};

use crate::sys::misc::ltp_cgroup_helper::{
    ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone,
    ltp_standalone_skip_blocking_fast_exit_if_needed,
};
use wateros_platform_arch_api_v0::trap::{SignalFrameCodec, SignalMachineContext, TrapFrameRead};

use super::kill_target::{
    can_signal, classify_kill_target, validate_thread_target, KillTargetSelector, SignalIdentity,
};
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const RT_SIGSET_SIZE_64 : usize = 8;
const RT_SIGACTION_SIZE : usize = 24;
const NSIG : usize = 64;
const SIGNAL_FRAME_MAGIC : u64 = 0x5741_5445_5253_4947;
const SS_ONSTACK : i32 = 1;
const SS_DISABLE : i32 = 2;
const MINSIGSTKSZ : usize = 2048;
static LAST_ACCOUNTING_NS : AtomicU64 = AtomicU64::new(0);

#[cfg(target_arch = "riscv64")]
const SIGNAL_TRAMPOLINE : usize = 0x0000_0000_7FFF_B000;
#[cfg(target_arch = "loongarch64")]
const SIGNAL_TRAMPOLINE : usize = 0x0000_007F_FFFF_B000;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserSigInfo {
    signo : i32,
    errno : i32,
    code : i32,
    payload : [u8; 116],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSignalStack {
    sp : usize,
    flags : i32,
    padding : u32,
    size : usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserUContext {
    flags : usize,
    link : usize,
    stack : UserSignalStack,
    sigmask : u64,
    reserved : [u64; 15],
    machine : SignalMachineContext,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct UserRtSignalFrame {
    info : UserSigInfo,
    ucontext : UserUContext,
    magic : u64,
}

// ── 新增结构体（来自原 task.rs 的 signal 相关类型）────────────

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSigAction {
    handler : usize,
    flags : usize,
    mask : u64,
}

const _ : () = assert!(core::mem::size_of::<UserSigAction>() == RT_SIGACTION_SIZE);
const _ : () = assert!(core::mem::size_of::<UserSigInfo>() == 128);

// ── 内部辅助 ────────────────────────────────────────────────

fn validate_signal(signal : isize) -> Result<usize, ErrNo> {
    if signal < 0 || signal as usize >= NSIG {
        Err(ErrNo::EINVAL)
    } else {
        Ok(signal as usize)
    }
}

fn send_thread(task_id : usize, signal : usize) -> Result<(), ErrNo> {
    if signal == 0 {
        return Ok(());
    }
    let dispatch =
        ipc::signal::send_thread(task_id, signal).map_err(|error| match error {
                                                     SignalError::NoSuchTask |
                                                     SignalError::NoSuchProcess => ErrNo::ESRCH,
                                                     _ => ErrNo::EINVAL,
                                                 })?;
    apply_signal_dispatch(dispatch, signal);
    Ok(())
}

// ── 公开 API ────────────────────────────────────────────────

pub(crate) fn ensure_current_signal_state() -> Result<task::ProcessTaskSnapshot, ErrNo> {
    let snapshot = task::current_process_task_snapshot().ok_or(ErrNo::ESRCH)?;
    ipc::signal::ensure_process(snapshot.pid.raw(),
                                snapshot.task_id,
                                snapshot.tid.raw(),
                                []).map_err(|_| ErrNo::ESRCH)?;
    Ok(snapshot)
}

pub(crate) fn ensure_process_signal_state(pid : ProcessId) -> Result<(), ErrNo> {
    let task_ids = task::task_ids_for_process(pid).ok_or(ErrNo::ESRCH)?;
    let mut descriptors = task_ids.into_iter()
                                  .filter_map(task::process_task_snapshot)
                                  .collect::<alloc::vec::Vec<_>>();
    descriptors.sort_by_key(|snapshot| snapshot.tid.raw());
    let leader = descriptors.first()
                            .copied()
                            .ok_or(ErrNo::ESRCH)?;
    ipc::signal::ensure_process(pid.raw(),
                        leader.task_id,
                        leader.tid.raw(),
                        descriptors.iter()
                                   .skip(1)
                                   .map(|snapshot| {
                                       (snapshot.task_id, snapshot.tid.raw())
                                   }))
        .map_err(|_| ErrNo::ESRCH)
}

pub(crate) fn apply_signal_dispatch(dispatch : SignalDispatch, signal : usize) {
    let Some(task_id) = dispatch.target_task_id else {
        return;
    };
    match dispatch.delivery {
        SignalDelivery::Ignored => {}
        SignalDelivery::Pending => {
            let _ = task::interrupt_task(task_id);
        }
        SignalDelivery::Stop => {
            if let Some(snapshot) = task::process_task_snapshot(task_id) {
                if task::mark_process_stopped(snapshot.pid, signal as u8).is_ok() {
                    task::stop_process_tasks(snapshot.pid);
                    notify_parent_sigchld(snapshot.pid);
                    task::wake_parent_child_waiters(snapshot.pid);
                }
            }
        }
        SignalDelivery::Continue => {
            if let Some(snapshot) = task::process_task_snapshot(task_id) {
                if task::mark_process_continued(snapshot.pid).is_ok() {
                    task::continue_process_tasks(snapshot.pid);
                    notify_parent_sigchld(snapshot.pid);
                    task::wake_parent_child_waiters(snapshot.pid);
                }
            }
        }
        SignalDelivery::Terminate => {
            let exit_code = crate::sys::task::wait::signal_terminate_exit_code(signal, task_id);
            if task::current_task_id() == Some(task_id) {
                if let Some(snapshot) = task::process_task_snapshot(task_id) {
                    notify_parent_sigchld(snapshot.pid);
                    on_thread_exit(task_id, snapshot.pid.raw(), true);
                    if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                        for member in task_ids {
                            crate::sys::task::wait::wake_clear_child_tid_for_task(member);
                            crate::sys::ipc::robust::robust_exit_cleanup(member);
                            crate::sys::task::wait::drop_task_runtime_resources(member);
                        }
                    }
                } else {
                    crate::sys::task::wait::wake_clear_child_tid_for_task(task_id);
                    crate::sys::ipc::robust::robust_exit_cleanup(task_id);
                    crate::sys::task::wait::drop_task_runtime_resources(task_id);
                }
                task::exit_group_current(exit_code);
            }
            if let Some(snapshot) = task::process_task_snapshot(task_id) {
                notify_parent_sigchld(snapshot.pid);
                if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                    for member in task_ids {
                        crate::sys::task::wait::wake_clear_child_tid_for_task(member);
                        crate::sys::ipc::robust::robust_exit_cleanup(member);
                        if task::kill_task(member, exit_code) {
                            crate::sys::task::wait::drop_task_runtime_resources(member);
                        }
                    }
                }
                ipc::signal::drop_process(snapshot.pid.raw());
            }
        }
    }
}

pub(crate) fn raise_current_thread(signal : usize) -> Result<(), ErrNo> {
    let snapshot = ensure_current_signal_state()?;
    send_thread(snapshot.task_id, signal)
}

pub(crate) fn notify_parent_sigchld(pid : ProcessId) {
    let Some(process) = task::process_snapshot(pid) else {
        return;
    };
    let Some(parent_pid) = process.parent_pid else {
        return;
    };
    if ensure_process_signal_state(parent_pid).is_err() {
        return;
    }
    if let Ok(dispatch) = ipc::signal::send_process(parent_pid.raw(), ipc::signal::SIGCHLD) {
        apply_signal_dispatch(dispatch, ipc::signal::SIGCHLD);
    }
}

pub(crate) fn timer_tick(interrupted_user : bool) {
    let now = match platform::wall_clock::monotonic_ns() {
        Ok(now) => now,
        Err(_) => return,
    };
    let now_u64 = u64::try_from(now).unwrap_or(u64::MAX);
    let previous = LAST_ACCOUNTING_NS.swap(now_u64, Ordering::Relaxed);
    let elapsed = if previous == 0 {
        (wateros_base_config::task::SCHED_TIMER_PERIOD_MS as u128) * 1_000_000
    } else {
        now_u64.saturating_sub(previous) as u128
    };

    let mut generated = alloc::vec::Vec::new();
    if let Ok(snapshot) = ensure_current_signal_state() {
        let user_delta = if interrupted_user { elapsed } else { 0 };
        if let Ok(cpu_signals) = ipc::signal::account_cpu(snapshot.pid.raw(), user_delta, elapsed) {
            generated.extend(cpu_signals);
        }
    }
    let realtime = ipc::signal::expire_realtime(now);
    generated.extend(realtime.into_iter()
                             .map(|dispatch| (dispatch, ipc::signal::SIGALRM)));
    if let Ok(realtime_now) = platform::wall_clock::realtime_ns() {
        generated.extend(ipc::signal::expire_posix_timers(now, realtime_now));
    }
    for (dispatch, signal) in generated {
        apply_signal_dispatch(dispatch, signal);
    }
}

pub(crate) fn abort_fork_signal(child_pid : usize, child_task_id : usize) {
    let _ = child_task_id;
    ipc::signal::drop_process(child_pid);
}

pub(crate) fn abort_clone_thread_signal(child_task_id : usize) {
    ipc::signal::drop_thread(child_task_id);
}

pub(crate) fn on_fork(parent_task_id : usize,
                      child_pid : usize,
                      child_task_id : usize,
                      child_tid : usize)
                      -> Result<(), SignalError> {
    ipc::signal::fork_process(parent_task_id,
                              child_pid,
                              child_task_id,
                              child_tid)
}

pub(crate) fn on_clone_thread(parent_task_id : usize,
                              child_task_id : usize,
                              child_tid : usize)
                              -> Result<(), SignalError> {
    ipc::signal::register_thread(parent_task_id, child_task_id, child_tid)
}

pub(crate) fn on_exec(task_id : usize, removed_threads : &[task::ExitedTask]) {
    let _ = ipc::signal::exec_process(task_id,
                                      removed_threads.iter()
                                                     .map(|thread| thread.id));
}

pub(crate) fn on_thread_exit(task_id : usize, pid : usize, last_thread : bool) {
    ipc::signal::exit_thread(task_id, pid, last_thread);
}

pub(crate) fn drop_thread_state(task_id : usize) {
    ipc::signal::drop_thread_and_empty_process(task_id);
}

// ── 信号递送与恢复 ──────────────────────────────────────────

pub(crate) fn deliver_pending_signal(frame : *mut u8,
                                     restart : Option<(usize, SyscallArgs)>)
                                     -> Result<bool, ErrNo> {
    let snapshot = ensure_current_signal_state()?;
    let pending = ipc::signal::take_deliverable(snapshot.task_id);
    let Some(pending) = pending else {
        return Ok(false);
    };
    if !pending.action
               .has_user_handler()
    {
        return Err(ErrNo::EINVAL);
    }

    let context = unsafe { &mut *(frame.cast::<ActiveTrapFrame>()) };
    let mut original = context.capture_signal_context();
    if pending.action.flags & ipc::signal::SA_RESTART != 0 {
        if let Some((syscall_nr, args)) = restart {
            ActiveTrapFrame::prepare_syscall_restart(&mut original,
                                                     syscall_nr,
                                                     args.as_regs(),
                                                     4);
        }
    }
    let alternate_stack = ipc::signal::alternate_stack(snapshot.task_id).map_err(|_| ErrNo::ESRCH)?;
    let interrupted_sp = TrapFrameRead::user_sp(context);
    let already_on_alternate =
        alternate_stack.active_frames != 0 || alternate_stack.contains(interrupted_sp);
    let switch_to_alternate = pending.action.flags & ipc::signal::SA_ONSTACK != 0 &&
                              alternate_stack.is_enabled() &&
                              !already_on_alternate;
    let stack_top = if switch_to_alternate {
        alternate_stack.sp
                       .checked_add(alternate_stack.size)
                       .ok_or(ErrNo::EFAULT)?
    } else {
        interrupted_sp
    };
    let frame_size = core::mem::size_of::<UserRtSignalFrame>();
    let frame_sp = stack_top.checked_sub(frame_size)
                            .map(|sp| sp & !0xF)
                            .ok_or(ErrNo::EFAULT)?;
    let frame_on_alternate = already_on_alternate || switch_to_alternate;
    let user_frame =
        UserRtSignalFrame { info : UserSigInfo { signo : pending.signal as i32,
                                                 errno : 0,
                                                 code : 0,
                                                 payload : [0; 116] },
                            ucontext:
                                UserUContext { flags : 0,
                                               link : 0,
                                               stack:
                                                   signal_stack_for_user(alternate_stack,
                                                                         already_on_alternate),
                                               sigmask : pending.previous_mask
                                                                .bits(),
                                               reserved : [0; 15],
                                               machine : original },
                            magic : SIGNAL_FRAME_MAGIC };
    copy_to_user_struct(frame_sp, &user_frame)?;
    ipc::signal::enter_signal_frame(snapshot.task_id, frame_on_alternate).map_err(|_| {
                                                                             ErrNo::ESRCH
                                                                         })?;

    let info_ptr = frame_sp;
    let ucontext_ptr = frame_sp + core::mem::offset_of!(UserRtSignalFrame, ucontext);
    let restorer = if pending.action
                             .restorer >
                      1 &&
                      pending.action
                             .restorer <
                      usize::MAX - 4096
    {
        pending.action
               .restorer
    } else {
        SIGNAL_TRAMPOLINE
    };
    context.prepare_signal_handler(pending.action
                                          .handler,
                                   restorer,
                                   frame_sp,
                                   pending.signal,
                                   info_ptr,
                                   ucontext_ptr);
    Ok(true)
}

pub(crate) fn restore_signal_frame(frame : *mut u8) -> Result<(), ErrNo> {
    let snapshot = ensure_current_signal_state()?;
    let context = unsafe { &mut *(frame.cast::<ActiveTrapFrame>()) };
    let frame_sp = TrapFrameRead::user_sp(context);
    let user_frame = copy_from_user_struct::<UserRtSignalFrame>(frame_sp)?;
    if user_frame.magic != SIGNAL_FRAME_MAGIC {
        return Err(ErrNo::EFAULT);
    }
    if !context.restore_signal_context(&user_frame.ucontext
                                                  .machine)
    {
        return Err(ErrNo::EFAULT);
    }
    ipc::signal::leave_signal_frame(snapshot.task_id,
                                    SignalSet::from_bits(user_frame.ucontext
                                                                   .sigmask),
                                    frame_sp).map_err(|_| ErrNo::ESRCH)
}

fn signal_stack_for_user(stack : ipc::signal::AlternateSignalStack,
                         on_stack : bool)
                         -> UserSignalStack {
    if !stack.is_enabled() {
        UserSignalStack { sp : 0,
                          flags : SS_DISABLE,
                          padding : 0,
                          size : 0 }
    } else {
        UserSignalStack { sp : stack.sp,
                          flags : if on_stack { SS_ONSTACK } else { 0 },
                          padding : 0,
                          size : stack.size }
    }
}

// ── syscall 实现 ────────────────────────────────────────────

pub(crate) fn sys_rt_sigpending(args : SyscallArgs) -> UserRet {
    let set_ptr = args.arg(0);
    let sigset_size = args.arg(1);
    if set_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let snapshot = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let pending = match ipc::signal::pending(snapshot.task_id) {
        Ok(pending) => pending,
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    };
    match copy_to_user_struct(set_ptr, &pending.bits()) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

pub(crate) fn sys_rt_sigprocmask(args : SyscallArgs) -> UserRet {
    let how = args.arg(0);
    let set = args.arg(1);
    let oldset = args.arg(2);
    let sigset_size = args.arg(3);

    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let new_set = if set == 0 {
        None
    } else {
        match copy_from_user_struct::<u64>(set) {
            Ok(bits) => Some(SignalSet::from_bits(bits)),
            Err(e) => return UserRet::from_error(e),
        }
    };
    let old = match ipc::signal::update_mask(task_id, how, new_set) {
        Ok(old) => old,
        Err(SignalError::InvalidHow) => return UserRet::from_error(ErrNo::EINVAL),
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };
    if oldset != 0 {
        if let Err(e) = copy_to_user_struct(oldset, &old.bits()) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_sigaltstack(args : SyscallArgs) -> UserRet {
    let new_stack_ptr = args.arg(0);
    let old_stack_ptr = args.arg(1);
    let task_id = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let current = match ipc::signal::alternate_stack(task_id) {
        Ok(stack) => stack,
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    };
    if old_stack_ptr != 0 {
        let old = signal_stack_for_user(current, current.active_frames != 0);
        if let Err(error) = copy_to_user_struct(old_stack_ptr, &old) {
            return UserRet::from_error(error);
        }
    }
    if new_stack_ptr == 0 {
        return UserRet::from_success(0);
    }
    let requested = match copy_from_user_struct::<UserSignalStack>(new_stack_ptr) {
        Ok(stack) => stack,
        Err(error) => return UserRet::from_error(error),
    };
    let replacement = match parse_signal_stack(requested) {
        Ok(stack) => stack,
        Err(error) => return UserRet::from_error(error),
    };
    match ipc::signal::replace_alternate_stack(task_id, replacement) {
        Ok(_) => UserRet::from_success(0),
        Err(SignalError::AlternateStackActive) => UserRet::from_error(ErrNo::EPERM),
        Err(_) => UserRet::from_error(ErrNo::ESRCH),
    }
}

fn parse_signal_stack(stack : UserSignalStack) -> Result<ipc::signal::AlternateSignalStack, ErrNo> {
    if stack.flags == SS_DISABLE {
        return Ok(ipc::signal::AlternateSignalStack::default());
    }
    if stack.flags != 0 {
        return Err(ErrNo::EINVAL);
    }
    if stack.size < MINSIGSTKSZ {
        return Err(ErrNo::ENOMEM);
    }
    if stack.sp == 0 ||
       stack.sp
            .checked_add(stack.size)
            .is_none()
    {
        return Err(ErrNo::EINVAL);
    }
    Ok(ipc::signal::AlternateSignalStack { sp : stack.sp,
                                           size : stack.size,
                                           active_frames : 0 })
}

pub(crate) fn sys_rt_sigsuspend(args : SyscallArgs) -> UserRet {
    let mask_ptr = args.arg(0);
    let sigset_size = args.arg(1);
    if mask_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let snapshot = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let bits = match copy_from_user_struct::<u64>(mask_ptr) {
        Ok(bits) => bits,
        Err(error) => return UserRet::from_error(error),
    };
    match ipc::signal::begin_sigsuspend(snapshot.task_id,
                                        SignalSet::from_bits(bits))
    {
        Ok(()) => {}
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    }
    ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone();
    ltp_standalone_skip_blocking_fast_exit_if_needed();
    let wait = task::wait_queue::WaitQueue::new_named("sigsuspend");
    let _ =
        wait.wait_current_while(|| !ipc::signal::has_deliverable(snapshot.task_id).unwrap_or(true));
    let _ = wait.try_release_empty();
    // Keep the temporary mask installed until the trap return path consumes the
    // pending signal. `take_deliverable` clears the suspend state and records
    // the original mask in the user signal frame for `rt_sigreturn`.
    UserRet::from_error(ErrNo::EINTR)
}

pub(crate) fn sys_rt_sigtimedwait(args : SyscallArgs) -> UserRet {
    let set = args.arg(0);
    let info = args.arg(1);
    let timeout = args.arg(2);
    let sigset_size = args.arg(3);

    if set == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let wait_set = match copy_from_user_struct::<u64>(set) {
        Ok(bits) => SignalSet::from_bits(bits),
        Err(e) => return UserRet::from_error(e),
    };
    let deadline = if timeout == 0 {
        None
    } else {
        let timeout = match copy_from_user_struct::<UserTimespec>(timeout) {
            Ok(timeout)
                if timeout.sec >= 0 && timeout.nsec >= 0 && timeout.nsec < 1_000_000_000 =>
            {
                timeout
            }
            Ok(_) => return UserRet::from_error(ErrNo::EINVAL),
            Err(error) => return UserRet::from_error(error),
        };
        let duration = (timeout.sec as u128).saturating_mul(1_000_000_000)
                                            .saturating_add(timeout.nsec as u128);
        let now = platform::wall_clock::monotonic_ns().unwrap_or(0);
        Some(now.saturating_add(duration))
    };
    if deadline.is_none() {
        ltp_standalone_skip_blocking_fast_exit_if_needed();
    }
    let wait_queue = task::wait_queue::WaitQueue::new_named("sigtimedwait");
    let sig = loop {
        if let Some(sig) = ipc::signal::take_pending(task_id, wait_set) {
            break sig;
        }
        let ticks = match deadline {
            Some(deadline) => {
                let now = platform::wall_clock::monotonic_ns().unwrap_or(deadline);
                if now >= deadline {
                    let _ = wait_queue.try_release_empty();
                    return UserRet::from_error(ErrNo::EAGAIN);
                }
                let tick_ns =
                    (wateros_base_config::task::SCHED_TIMER_PERIOD_MS as u128) * 1_000_000;
                u64::try_from((deadline - now).saturating_add(tick_ns - 1) / tick_ns)
                    .unwrap_or(u64::MAX)
                    .max(1)
            }
            None => u64::MAX,
        };
        let _ = ipc::signal::begin_signal_wait(task_id, wait_set);
        let still_waiting = || {
            ipc::signal::pending_in(task_id, wait_set).map(|has_pending| !has_pending)
                                                      .unwrap_or(false)
        };
        let wait_result = if deadline.is_some() {
            wait_queue.wait_current_while_for_ticks(ticks, still_waiting)
        } else {
            wait_queue.wait_current_while(still_waiting)
        };
        let _ = ipc::signal::end_signal_wait(task_id);
        if wait_result == task::TaskWaitResult::Interrupted {
            if let Some(sig) = ipc::signal::take_pending(task_id, wait_set) {
                break sig;
            }
            let _ = wait_queue.try_release_empty();
            return UserRet::from_error(ErrNo::EINTR);
        }
    };
    let _ = wait_queue.try_release_empty();
    if info != 0 {
        let siginfo = UserSigInfo { signo : sig as i32,
                                    errno : 0,
                                    code : 0,
                                    payload : [0; 116] };
        if let Err(e) = copy_to_user_struct(info, &siginfo) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(sig)
}

pub(crate) fn sys_rt_sigaction(args : SyscallArgs) -> UserRet {
    let sig = args.arg(0);
    let act = args.arg(1);
    let oldact = args.arg(2);
    let sigset_size = args.arg(3);

    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot.task_id,
        Err(error) => return UserRet::from_error(error),
    };
    let old = match ipc::signal::get_action(task_id, sig) {
        Ok(old) => old,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };
    if oldact != 0 {
        let user_old = UserSigAction { handler : old.handler,
                                       flags : old.flags,
                                       mask : old.mask.bits() };
        if let Err(e) = copy_to_user_struct(oldact, &user_old) {
            return UserRet::from_error(e);
        }
    }
    if act != 0 {
        let user_action = match copy_from_user_struct::<UserSigAction>(act) {
            Ok(action) => action,
            Err(e) => return UserRet::from_error(e),
        };
        let action = ipc::signal::SignalAction { handler : user_action.handler,
                                                 flags : user_action.flags,
                                                 restorer : 0,
                                                 mask : SignalSet::from_bits(user_action.mask) };
        match ipc::signal::set_action(task_id, sig, action) {
            Ok(_) => {}
            Err(_) => return UserRet::from_error(ErrNo::EINVAL),
        }
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_tkill(args : SyscallArgs) -> UserRet {
    let tid = match validate_thread_target(args.arg(0)) {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let signal = match validate_signal(args.arg(1) as isize) {
        Ok(signal) => signal,
        Err(error) => return UserRet::from_error(error),
    };
    let task_id = match task::task_id_for_thread(ThreadId::from_raw(tid)) {
        Some(task_id) => task_id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let snapshot = match task::process_task_snapshot(task_id) {
        Some(snapshot) => snapshot,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if let Err(error) = check_signal_permission(snapshot.pid, signal) {
        return UserRet::from_error(error);
    }
    if ensure_process_signal_state(snapshot.pid).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    match send_thread(task_id, signal) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

pub(crate) fn sys_tgkill(args : SyscallArgs) -> UserRet {
    let tgid = match validate_thread_target(args.arg(0)) {
        Some(tgid) => tgid,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let tid = match validate_thread_target(args.arg(1)) {
        Some(tid) => tid,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let signal = match validate_signal(args.arg(2) as isize) {
        Ok(signal) => signal,
        Err(error) => return UserRet::from_error(error),
    };
    let task_id = match task::task_id_for_thread(ThreadId::from_raw(tid)) {
        Some(task_id) => task_id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    let snapshot = match task::process_task_snapshot(task_id) {
        Some(snapshot) if snapshot.pid.raw() == tgid => snapshot,
        _ => return UserRet::from_error(ErrNo::ESRCH),
    };
    if let Err(error) = check_signal_permission(snapshot.pid, signal) {
        return UserRet::from_error(error);
    }
    if ensure_process_signal_state(snapshot.pid).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    match send_thread(task_id, signal) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

// ── kill(2)（来自 kill.rs）─────────────────────────────────

const _NSIG : i32 = 64;

fn resolve_process_group_targets(pgid : ProcessId) -> Result<Vec<ProcessId>, ErrNo> {
    let targets = task::process_pids_in_pgid(pgid);
    if targets.is_empty() {
        Err(ErrNo::ESRCH)
    } else {
        Ok(targets)
    }
}

fn resolve_kill_targets(pid : isize) -> Result<Vec<ProcessId>, ErrNo> {
    match classify_kill_target(pid) {
        KillTargetSelector::Process(pid) => Ok(alloc::vec![ProcessId::from_raw(pid)]),
        KillTargetSelector::CurrentProcessGroup => {
            let current = task::current_process_snapshot().ok_or(ErrNo::ESRCH)?;
            resolve_process_group_targets(current.pgid)
        }
        KillTargetSelector::Broadcast => {
            let current = task::current_process_snapshot().ok_or(ErrNo::ESRCH)?;
            Ok(task::all_process_pids().into_iter()
                                       .filter(|p| *p != current.pid && p.raw() != 1)
                                       .collect())
        }
        KillTargetSelector::ProcessGroup(pgid) => {
            resolve_process_group_targets(ProcessId::from_raw(pgid))
        }
    }
}

fn signal_identity(credentials : cred::ProcessCredentials,
                   session_id : ProcessId)
                   -> SignalIdentity {
    SignalIdentity { real_uid : credentials.real_uid
                                           .0,
                     effective_uid : credentials.effective_uid
                                                .0,
                     saved_uid : credentials.saved_uid
                                            .0,
                     session_id : session_id.raw() }
}

fn check_signal_permission(process : ProcessId, sig : usize) -> Result<(), ErrNo> {
    let caller_process = task::current_process_snapshot().ok_or(ErrNo::ESRCH)?;
    let target_process = task::process_snapshot(process).ok_or(ErrNo::ESRCH)?;
    let target_credentials =
        cred::try_credentials_for(target_process.leader_task_id).ok_or(ErrNo::ESRCH)?;
    let caller_credentials = cred::current_credentials();
    let privileged = caller_credentials.effective_uid
                                       .0 ==
                     0;

    if can_signal(signal_identity(caller_credentials, caller_process.sid),
                  signal_identity(target_credentials, target_process.sid),
                  sig,
                  privileged)
    {
        Ok(())
    } else {
        Err(ErrNo::EPERM)
    }
}

fn send_signal_to_process(process : ProcessId, sig : usize) -> Result<(), ErrNo> {
    if task::leader_task_for_process(process).is_none() {
        return Err(ErrNo::ESRCH);
    }
    check_signal_permission(process, sig)?;
    if ensure_process_signal_state(process).is_err() {
        return Err(ErrNo::ESRCH);
    }
    let dispatch = ipc::signal::send_process(process.raw(), sig).map_err(|_| ErrNo::EINVAL)?;
    apply_signal_dispatch(dispatch, sig);
    if dispatch.delivery == SignalDelivery::Pending {
        if let Some(task_ids) = task::task_ids_for_process(process) {
            for member in task_ids {
                let deliverable = ipc::signal::has_deliverable(member).unwrap_or(false);
                if deliverable {
                    let _ = task::interrupt_task(member);
                }
            }
        }
    }
    Ok(())
}

/// `kill(pid, sig)` — riscv64 系统调用号 129。
pub(crate) fn sys_kill(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let sig = args.arg(1) as i32;

    if sig < 0 || sig >= _NSIG {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let targets = match resolve_kill_targets(pid) {
        Ok(targets) => targets,
        Err(e) => return UserRet::from_error(e),
    };
    if targets.is_empty() {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    if sig == 0 {
        let mut found = false;
        for process in targets {
            match check_signal_permission(process, 0) {
                Ok(()) => return UserRet::from_success(0),
                Err(ErrNo::EPERM) => found = true,
                Err(_) => {}
            }
        }
        if found {
            return UserRet::from_error(ErrNo::EPERM);
        }
        return UserRet::from_error(ErrNo::ESRCH);
    }

    let mut sent = false;
    let mut last_err = ErrNo::ESRCH;
    for process in targets {
        match send_signal_to_process(process, sig as usize) {
            Ok(()) => sent = true,
            Err(e) => last_err = e,
        }
    }
    if sent {
        UserRet::from_success(0)
    } else {
        UserRet::from_error(last_err)
    }
}
