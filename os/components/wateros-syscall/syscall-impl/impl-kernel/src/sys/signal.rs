//! Process/thread signal routing and Linux signal syscall helpers.

use core::sync::atomic::{AtomicU64, Ordering};

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::{SignalDelivery, SignalDispatch, SignalError, SignalSet};
use platform::arch::trap::ActiveTrapFrame;
use task::{ProcessId, ThreadId};

use super::ltp_cgroup_helper::{
    ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone,
    ltp_standalone_skip_blocking_fast_exit_if_needed,
};
use wateros_platform_arch_api_v0::trap::{SignalFrameCodec, SignalMachineContext, TrapFrameRead};

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const RT_SIGSET_SIZE_64 : usize = 8;
const NSIG : usize = 64;
const SIGNAL_FRAME_MAGIC : u64 = 0x5741_5445_5253_4947;
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

pub(crate) fn ensure_current_signal_state() -> Result<task::ProcessTaskDescriptor, ErrNo> {
    let snapshot = task::current_process_task_snapshot().ok_or(ErrNo::ESRCH)?;
    ipc::signal::with_registry(|registry| {
        if !registry.has_thread(snapshot.task_id) {
            registry.register_process(snapshot.pid.raw(),
                                      snapshot.task_id,
                                      snapshot.tid.raw());
        }
    });
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
    ipc::signal::with_registry(|registry| {
        if !registry.has_process(pid.raw()) {
            registry.register_process(pid.raw(),
                                      leader.task_id,
                                      leader.tid.raw());
        }
        for snapshot in descriptors.iter()
                                   .skip(1)
        {
            if !registry.has_thread(snapshot.task_id) {
                let _ = registry.register_thread(leader.task_id,
                                                 snapshot.task_id,
                                                 snapshot.tid.raw());
            }
        }
    });
    Ok(())
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
                if task::mark_process_stopped(snapshot.pid, signal as u8) {
                    task::stop_process_tasks(snapshot.pid);
                    notify_parent_sigchld(snapshot.pid);
                    task::wake_parent_child_waiters(snapshot.pid);
                }
            }
        }
        SignalDelivery::Continue => {
            if let Some(snapshot) = task::process_task_snapshot(task_id) {
                if task::mark_process_continued(snapshot.pid) {
                    task::continue_process_tasks(snapshot.pid);
                    notify_parent_sigchld(snapshot.pid);
                    task::wake_parent_child_waiters(snapshot.pid);
                }
            }
        }
        SignalDelivery::Terminate => {
            let exit_code = super::task::signal_terminate_exit_code(signal, task_id);
            if task::current_task_id() == Some(task_id) {
                if let Some(snapshot) = task::process_task_snapshot(task_id) {
                    notify_parent_sigchld(snapshot.pid);
                    on_thread_exit(task_id, snapshot.pid.raw(), true);
                }
                super::task::wake_clear_child_tid_for_task(task_id);
                super::robust::robust_exit_cleanup(task_id);
                task::exit_group_current(exit_code);
            }
            if let Some(snapshot) = task::process_task_snapshot(task_id) {
                notify_parent_sigchld(snapshot.pid);
                if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                    for member in task_ids {
                        super::task::wake_clear_child_tid_for_task(member);
                        super::robust::robust_exit_cleanup(member);
                        let _ = task::kill_task(member, exit_code);
                    }
                }
                ipc::signal::with_registry(|registry| registry.drop_process(snapshot.pid.raw()));
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
    if let Ok(dispatch) = ipc::signal::with_registry(|registry| {
        registry.send_process(parent_pid.raw(), ipc::signal::SIGCHLD)
    }) {
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
        if let Ok(cpu_signals) = ipc::signal::with_registry(|registry| {
            registry.account_cpu(snapshot.pid.raw(), user_delta, elapsed)
        }) {
            generated.extend(cpu_signals);
        }
    }
    let realtime = ipc::signal::with_registry(|registry| registry.expire_realtime(now));
    generated.extend(realtime.into_iter()
                             .map(|dispatch| (dispatch, ipc::signal::SIGALRM)));
    for (dispatch, signal) in generated {
        apply_signal_dispatch(dispatch, signal);
    }
}

pub(crate) fn abort_fork_signal(child_pid : usize, child_task_id : usize) {
    let _ = child_task_id;
    ipc::signal::with_registry(|registry| registry.drop_process(child_pid));
}

pub(crate) fn abort_clone_thread_signal(child_task_id : usize) {
    ipc::signal::with_registry(|registry| registry.drop_thread(child_task_id));
}

pub(crate) fn on_fork(parent_task_id : usize,
                      child_pid : usize,
                      child_task_id : usize,
                      child_tid : usize)
                      -> Result<(), SignalError> {
    ipc::signal::with_registry(|registry| {
        registry.fork_process(parent_task_id,
                              child_pid,
                              child_task_id,
                              child_tid)
    })
}

pub(crate) fn on_clone_thread(parent_task_id : usize,
                              child_task_id : usize,
                              child_tid : usize)
                              -> Result<(), SignalError> {
    ipc::signal::with_registry(|registry| {
        registry.register_thread(parent_task_id, child_task_id, child_tid)
    })
}

pub(crate) fn on_exec(task_id : usize, removed_threads : &[task::ExitedTask]) {
    ipc::signal::with_registry(|registry| {
        for thread in removed_threads {
            registry.drop_thread(thread.id);
        }
        let _ = registry.exec_process(task_id);
    });
}

pub(crate) fn on_thread_exit(task_id : usize, pid : usize, last_thread : bool) {
    ipc::signal::with_registry(|registry| {
        registry.drop_thread(task_id);
        if last_thread {
            registry.drop_process(pid);
        }
    });
}

pub(crate) fn drop_thread_state(task_id : usize) {
    ipc::signal::with_registry(|registry| {
        registry.drop_thread_and_empty_process(task_id);
    });
}

pub(crate) fn deliver_pending_signal(frame : *mut u8,
                                     restart : Option<(usize, SyscallArgs)>)
                                     -> Result<bool, ErrNo> {
    let snapshot = ensure_current_signal_state()?;
    let pending =
        ipc::signal::with_registry(|registry| registry.take_deliverable(snapshot.task_id));
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
    let frame_size = core::mem::size_of::<UserRtSignalFrame>();
    let frame_sp = TrapFrameRead::user_sp(context).checked_sub(frame_size)
                                                  .map(|sp| sp & !0xF)
                                                  .ok_or(ErrNo::EFAULT)?;
    let user_frame =
        UserRtSignalFrame { info : UserSigInfo { signo : pending.signal as i32,
                                                 errno : 0,
                                                 code : 0,
                                                 payload : [0; 116] },
                            ucontext : UserUContext { flags : 0,
                                                      link : 0,
                                                      stack : UserSignalStack::default(),
                                                      sigmask : pending.previous_mask
                                                                       .bits(),
                                                      reserved : [0; 15],
                                                      machine : original },
                            magic : SIGNAL_FRAME_MAGIC };
    copy_to_user_struct(frame_sp, &user_frame)?;

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
    let user_frame = copy_from_user_struct::<UserRtSignalFrame>(TrapFrameRead::user_sp(context))?;
    if user_frame.magic != SIGNAL_FRAME_MAGIC {
        return Err(ErrNo::EFAULT);
    }
    if !context.restore_signal_context(&user_frame.ucontext
                                                  .machine)
    {
        return Err(ErrNo::EFAULT);
    }
    ipc::signal::with_registry(|registry| {
        registry.restore_mask(snapshot.task_id,
                              SignalSet::from_bits(user_frame.ucontext
                                                             .sigmask))
    }).map_err(|_| ErrNo::ESRCH)
}

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
    let dispatch = ipc::signal::with_registry(|registry| registry.send_thread(task_id, signal))
        .map_err(|error| match error {
            SignalError::NoSuchTask | SignalError::NoSuchProcess => ErrNo::ESRCH,
            _ => ErrNo::EINVAL,
        })?;
    apply_signal_dispatch(dispatch, signal);
    Ok(())
}

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
    let pending = match ipc::signal::with_registry(|registry| registry.pending(snapshot.task_id)) {
        Ok(pending) => pending,
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    };
    match copy_to_user_struct(set_ptr, &pending.bits()) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
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
    match ipc::signal::with_registry(|registry| {
              registry.begin_sigsuspend(snapshot.task_id,
                                        SignalSet::from_bits(bits))
          }) {
        Ok(()) => {}
        Err(_) => return UserRet::from_error(ErrNo::ESRCH),
    }
    ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone();
    ltp_standalone_skip_blocking_fast_exit_if_needed();
    let wait = task::wait_queue::WaitQueue::new();
    let _ = wait.wait_current_while(|| {
                    ipc::signal::with_registry(|registry| {
                        !registry.has_deliverable(snapshot.task_id)
                                 .unwrap_or(true)
                    })
                });
    let _ = wait.try_release_empty();
    let _ = ipc::signal::with_registry(|registry| registry.end_sigsuspend(snapshot.task_id));
    UserRet::from_error(ErrNo::EINTR)
}

pub(crate) fn sys_tkill(args : SyscallArgs) -> UserRet {
    let tid = args.arg(0);
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
    if ensure_process_signal_state(snapshot.pid).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    match send_thread(task_id, signal) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

pub(crate) fn sys_tgkill(args : SyscallArgs) -> UserRet {
    let tgid = args.arg(0);
    let tid = args.arg(1);
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
    if ensure_process_signal_state(snapshot.pid).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    match send_thread(task_id, signal) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}
