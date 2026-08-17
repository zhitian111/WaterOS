//! 进程/线程信号路由与 Linux 信号类 syscall 辅助逻辑。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use ipc::signal::{
    PendingSignalScope, SignalDelivery, SignalDispatch, SignalEffect, SignalError, SignalSet,
};
use platform::arch::trap::ActiveTrapFrame;
use task::{ProcessId, ThreadId};

use wateros_platform_arch_api_v0::trap::{SignalFrameCodec, SignalMachineContext, TrapFrameRead};

use super::kill_target::{
    can_signal, classify_kill_target, validate_thread_target, KillTargetSelector, SignalIdentity,
};
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const RT_SIGSET_SIZE_64 : usize = 8;
const RT_SIGACTION_SIZE : usize = 24;
const NSIG : usize = 64;
const SS_ONSTACK : i32 = 1;
const SS_DISABLE : i32 = 2;
#[cfg(target_arch = "riscv64")]
const MINSIGSTKSZ : usize = 2048;
#[cfg(target_arch = "loongarch64")]
const MINSIGSTKSZ : usize = 4096;
static LAST_ACCOUNTING_NS : [AtomicU64; wateros_base_config::task::MAX_CPUS] =
    [const { AtomicU64::new(0) }; wateros_base_config::task::MAX_CPUS];

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct PendingSignalSource {
    pub(super) pid : usize,
    pub(super) uid : u32,
    code : i32,
    fault_addr : usize,
}

/// pending 位图的所有者。线程定向信号必须使用 task ID，不能与同进程的
/// 其它线程共用 PID 键。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PendingSignalOwner {
    Thread(usize),
    Process(usize),
}

/// 补充 pending 位图不保存的 siginfo 来源：用户信号的 pid/uid，以及同步 CPU
/// 异常的正 `si_code`/`si_addr`。
static PENDING_SIGNAL_SOURCES : Mutex<BTreeMap<(PendingSignalOwner, usize), PendingSignalSource>> =
    Mutex::new(BTreeMap::new());

fn record_pending_signal_source(owner : PendingSignalOwner,
                                signal : usize,
                                source : PendingSignalSource) {
    if signal != 0 {
        PENDING_SIGNAL_SOURCES.lock()
                              .insert((owner, signal), source);
    }
}

fn pending_signal_owner(task_id : usize,
                        process_pid : usize,
                        scope : PendingSignalScope)
                        -> PendingSignalOwner {
    match scope {
        PendingSignalScope::Thread => PendingSignalOwner::Thread(task_id),
        PendingSignalScope::Process => PendingSignalOwner::Process(process_pid),
    }
}

pub(super) fn take_pending_signal_source(task_id : usize,
                                         process_pid : usize,
                                         scope : PendingSignalScope,
                                         signal : usize)
                                         -> PendingSignalSource {
    PENDING_SIGNAL_SOURCES.lock()
                          .remove(&(pending_signal_owner(task_id, process_pid, scope), signal))
                          .unwrap_or_default()
}

pub(super) fn peek_pending_signal_source(task_id : usize,
                                         process_pid : usize,
                                         scope : PendingSignalScope,
                                         signal : usize)
                                         -> PendingSignalSource {
    PENDING_SIGNAL_SOURCES.lock()
                          .get(&(pending_signal_owner(task_id, process_pid, scope), signal))
                          .copied()
                          .unwrap_or_default()
}

fn take_delivery_signal_source(task_id : usize,
                               process_pid : usize,
                               signal : usize)
                               -> PendingSignalSource {
    let mut sources = PENDING_SIGNAL_SOURCES.lock();
    sources.remove(&(PendingSignalOwner::Thread(task_id), signal))
           .or_else(|| sources.remove(&(PendingSignalOwner::Process(process_pid), signal)))
           .unwrap_or_default()
}

fn drop_thread_signal_sources(task_id : usize) {
    PENDING_SIGNAL_SOURCES.lock()
                          .retain(|(owner, _), _| *owner != PendingSignalOwner::Thread(task_id));
}

fn drop_process_signal_sources(process_pid : usize) {
    PENDING_SIGNAL_SOURCES.lock()
                          .retain(|(owner, _), _| *owner != PendingSignalOwner::Process(process_pid));
}

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
    pad : i32,
    payload : [u8; 112],
}

fn user_siginfo(signo : usize, source : PendingSignalSource) -> UserSigInfo {
    let mut payload = [0u8; 112];
    if source.code > 0 {
        // Linux siginfo_t places si_addr at the beginning of _sifields._sigfault.
        payload[..core::mem::size_of::<usize>()]
            .copy_from_slice(&source.fault_addr.to_ne_bytes());
    } else {
        // SI_USER/SI_TKILL use the _kill layout: pid followed by uid.
        payload[0..4].copy_from_slice(&(source.pid as u32).to_ne_bytes());
        payload[4..8].copy_from_slice(&source.uid.to_ne_bytes());
    }
    UserSigInfo { signo : signo as i32,
                  errno : 0,
                  code : source.code,
                  pad : 0,
                  payload }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserSignalStack {
    sp : usize,
    flags : i32,
    padding : u32,
    size : usize,
}

/// Linux keeps room for a 1024-bit signal mask even though the kernel ABI used by
/// `rt_sigprocmask` currently exposes one 64-bit word.
const USER_SIGMASK_PADDING : usize = 120;

#[cfg(target_arch = "riscv64")]
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct LinuxMachineContext {
    pc : usize,
    /// Linux omits x0 and stores x1..x31 in order.
    gprs : [usize; 31],
    fpregs : [u64; 32],
    fcsr : u32,
    /// Tail of Linux's 0x210-byte FP union.  The D-extension `fcsr` remains
    /// immediately after its 32 registers; the larger Q member only reserves
    /// the remaining storage and must be zero on signal return.
    fp_union_tail : [u8; 0x10c],
}

#[cfg(target_arch = "loongarch64")]
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct LinuxMachineContext {
    pc : usize,
    gprs : [usize; 32],
    flags : u32,
    padding : [u8; 4],
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct UserUContext {
    flags : usize,
    link : usize,
    stack : UserSignalStack,
    sigmask : u64,
    reserved : [u8; USER_SIGMASK_PADDING],
    machine : LinuxMachineContext,
}

#[cfg(target_arch = "riscv64")]
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct UserRtSignalFrame {
    info : UserSigInfo,
    ucontext : UserUContext,
}

#[cfg(target_arch = "loongarch64")]
const LOONGARCH_SC_USED_FP : u32 = 1;
#[cfg(target_arch = "loongarch64")]
const LOONGARCH_LSX_CTX_MAGIC : u32 = 0x5358_0001;

#[cfg(target_arch = "loongarch64")]
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
struct LoongArchContextInfo {
    magic : u32,
    size : u32,
    padding : u64,
}

#[cfg(target_arch = "loongarch64")]
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct LoongArchLsxContext {
    regs : [u64; 64],
    fcc : u64,
    fcsr : u32,
    padding : u32,
}

#[cfg(target_arch = "loongarch64")]
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct UserRtSignalFrame {
    info : UserSigInfo,
    ucontext : UserUContext,
    lsx_info : LoongArchContextInfo,
    lsx : LoongArchLsxContext,
    end : LoongArchContextInfo,
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
const _ : () = assert!(core::mem::offset_of!(UserRtSignalFrame, ucontext) == 0x80);
const _ : () = assert!(core::mem::offset_of!(UserRtSignalFrame, ucontext.machine) == 0x130);
#[cfg(target_arch = "riscv64")]
const _ : () = assert!(core::mem::size_of::<LinuxMachineContext>() == 0x310);
#[cfg(target_arch = "riscv64")]
const _ : () = assert!(core::mem::size_of::<UserRtSignalFrame>() == 0x440);
#[cfg(target_arch = "loongarch64")]
const _ : () = assert!(core::mem::size_of::<LinuxMachineContext>() == 0x110);
#[cfg(target_arch = "loongarch64")]
const _ : () = assert!(core::mem::offset_of!(UserRtSignalFrame, lsx_info) == 0x240);
#[cfg(target_arch = "loongarch64")]
const _ : () = assert!(core::mem::size_of::<LoongArchContextInfo>() == 0x10);
#[cfg(target_arch = "loongarch64")]
const _ : () = assert!(core::mem::size_of::<LoongArchLsxContext>() == 0x210);
#[cfg(target_arch = "loongarch64")]
const _ : () = assert!(core::mem::size_of::<UserRtSignalFrame>() == 0x470);

#[cfg(target_arch = "riscv64")]
fn encode_machine_context(context : &SignalMachineContext) -> LinuxMachineContext {
    let mut gprs = [0; 31];
    gprs.copy_from_slice(&context.gprs[1..]);
    LinuxMachineContext { pc : context.pc,
                          gprs,
                          fpregs : context.fpregs,
                          fcsr : context.fcsr,
                          fp_union_tail : [0; 0x10c] }
}

#[cfg(target_arch = "riscv64")]
fn decode_machine_context(context : &LinuxMachineContext) -> SignalMachineContext {
    let mut decoded = SignalMachineContext::default();
    decoded.pc = context.pc;
    decoded.gprs[1..].copy_from_slice(&context.gprs);
    decoded.fpregs = context.fpregs;
    decoded.fcsr = context.fcsr;
    decoded
}

#[cfg(target_arch = "loongarch64")]
fn encode_machine_context(context : &SignalMachineContext) -> LinuxMachineContext {
    LinuxMachineContext { pc : context.pc,
                          gprs : context.gprs,
                          flags : LOONGARCH_SC_USED_FP,
                          padding : [0; 4] }
}

#[cfg(target_arch = "loongarch64")]
fn encode_lsx_context(context : &SignalMachineContext) -> LoongArchLsxContext {
    let mut regs = [0; 64];
    for (slot, vector) in regs.chunks_exact_mut(2)
                              .zip(context.vectors.iter())
    {
        slot.copy_from_slice(vector);
    }
    LoongArchLsxContext { regs,
                          fcc : context.fcc,
                          fcsr : context.fcsr,
                          padding : 0 }
}

#[cfg(target_arch = "loongarch64")]
fn decode_machine_context(context : &LinuxMachineContext,
                          lsx : &LoongArchLsxContext)
                          -> SignalMachineContext {
    let mut decoded = SignalMachineContext::default();
    decoded.pc = context.pc;
    decoded.gprs = context.gprs;
    decoded.fcc = lsx.fcc;
    decoded.fcsr = lsx.fcsr;
    for (vector, slot) in decoded.vectors.iter_mut()
                                         .zip(lsx.regs.chunks_exact(2))
    {
        vector.copy_from_slice(slot);
    }
    for (fpreg, vector) in decoded.fpregs.iter_mut()
                                           .zip(decoded.vectors.iter())
    {
        *fpreg = vector[0];
    }
    decoded
}

#[cfg(target_arch = "riscv64")]
fn build_user_signal_frame(info : UserSigInfo,
                           stack : UserSignalStack,
                           mask : u64,
                           context : &SignalMachineContext)
                           -> UserRtSignalFrame {
    UserRtSignalFrame { info,
                        ucontext:
                            UserUContext { flags : 0,
                                           link : 0,
                                           stack,
                                           sigmask : mask,
                                           reserved : [0; USER_SIGMASK_PADDING],
                                           machine : encode_machine_context(context) } }
}

#[cfg(target_arch = "loongarch64")]
fn build_user_signal_frame(info : UserSigInfo,
                           stack : UserSignalStack,
                           mask : u64,
                           context : &SignalMachineContext)
                           -> UserRtSignalFrame {
    UserRtSignalFrame { info,
                        ucontext:
                            UserUContext { flags : 0,
                                           link : 0,
                                           stack,
                                           sigmask : mask,
                                           reserved : [0; USER_SIGMASK_PADDING],
                                           machine : encode_machine_context(context) },
                        lsx_info:
                            LoongArchContextInfo { magic : LOONGARCH_LSX_CTX_MAGIC,
                                                   size : (core::mem::size_of::<
                                                       LoongArchContextInfo,
                                                   >() +
                                                           core::mem::size_of::<
                                                               LoongArchLsxContext,
                                                           >()) as u32,
                                                   padding : 0 },
                        lsx : encode_lsx_context(context),
                        end : LoongArchContextInfo::default() }
}

#[cfg(target_arch = "riscv64")]
fn restore_user_machine_context(frame : &UserRtSignalFrame) -> Option<SignalMachineContext> {
    Some(decode_machine_context(&frame.ucontext.machine))
}

#[cfg(target_arch = "loongarch64")]
fn restore_user_machine_context(frame : &UserRtSignalFrame) -> Option<SignalMachineContext> {
    let expected_size = (core::mem::size_of::<LoongArchContextInfo>() +
                         core::mem::size_of::<LoongArchLsxContext>()) as u32;
    if frame.lsx_info.magic != LOONGARCH_LSX_CTX_MAGIC ||
       frame.lsx_info.size != expected_size ||
       frame.end.magic != 0 ||
       frame.end.size != 0
    {
        return None;
    }
    Some(decode_machine_context(&frame.ucontext.machine, &frame.lsx))
}

// ── 内部辅助 ────────────────────────────────────────────────

fn validate_signal(signal : isize) -> Result<usize, ErrNo> {
    if signal < 0 || signal as usize > NSIG {
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
            task::request_task_reschedule(task_id);
        }
        SignalDelivery::Stop => {
            if let Some(snapshot) = task::process_task_snapshot(task_id) {
                if task::mark_process_stopped(snapshot.pid, signal as u8).is_ok() {
                    notify_parent_sigchld(snapshot.pid);
                    task::wake_parent_child_waiters(snapshot.pid);
                }
                if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                    for member in task_ids {
                        task::request_task_reschedule(member);
                    }
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
                // SIGCONT 的恢复副作用不受 mask/disposition 影响；若安装了 handler，
                // registry 同时保留 pending，这里再唤醒一个可投递线程。
                if ipc::signal::has_deliverable(task_id).unwrap_or(false) {
                    let _ = task::interrupt_task(task_id);
                    task::request_task_reschedule(task_id);
                }
            }
        }
    }
}

pub(crate) fn raise_current_thread(signal : usize) -> Result<(), ErrNo> {
    let snapshot = ensure_current_signal_state()?;
    send_thread(snapshot.task_id, signal)
}

/// Queue a synchronous kernel fault with Linux-compatible `siginfo_t` fields.
///
/// Positive `si_code` plus `si_addr` are required by runtimes such as HotSpot;
/// reporting a CPU exception as `SI_USER` makes their chained fault handler
/// misclassify it as an asynchronously sent signal.
pub(crate) fn raise_current_fault_signal(signal : usize,
                                         code : i32,
                                         fault_addr : usize)
                                         -> Result<(), ErrNo> {
    if code <= 0 {
        return Err(ErrNo::EINVAL);
    }
    let snapshot = ensure_current_signal_state()?;
    let dispatch = ipc::signal::force_thread_signal(snapshot.task_id, signal)
        .map_err(|error| match error {
            SignalError::NoSuchTask | SignalError::NoSuchProcess => ErrNo::ESRCH,
            _ => ErrNo::EINVAL,
        })?;
    record_pending_signal_source(PendingSignalOwner::Thread(snapshot.task_id),
                                 signal,
                                 PendingSignalSource { code,
                                                       fault_addr,
                                                       ..PendingSignalSource::default() });
    apply_signal_dispatch(dispatch, signal);
    Ok(())
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
    let cpu_id = platform::arch::cpu::current_cpu_id().raw();
    let previous = LAST_ACCOUNTING_NS[cpu_id].swap(now_u64, Ordering::Relaxed);
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
    for thread in removed_threads {
        drop_thread_signal_sources(thread.id);
    }
    let _ = ipc::signal::exec_process(task_id,
                                      removed_threads.iter()
                                                     .map(|thread| thread.id));
}

pub(crate) fn on_thread_exit(task_id : usize, pid : usize, last_thread : bool) {
    drop_thread_signal_sources(task_id);
    if last_thread {
        drop_process_signal_sources(pid);
    }
    ipc::signal::exit_thread(task_id, pid, last_thread);
}

pub(crate) fn drop_thread_state(task_id : usize) {
    drop_thread_signal_sources(task_id);
    ipc::signal::drop_thread_and_empty_process(task_id);
}

// ── 信号递送与恢复 ──────────────────────────────────────────

pub(crate) fn deliver_pending_signal(frame : *mut u8,
                                     restart : Option<(usize, SyscallArgs)>)
                                     -> Result<bool, ErrNo> {
    if let Some(process) = task::current_process_snapshot() {
        match process.state {
            task::ProcessState::Stopped { .. } => {
                if task::current_task_id().is_some_and(ipc::signal::take_sigkill) {
                    let task_id = task::current_task_id().ok_or(ErrNo::ESRCH)?;
                    let exit_code =
                        crate::sys::task::wait::signal_terminate_exit_code(ipc::signal::SIGKILL,
                                                                           task_id);
                    crate::sys::task::exit_group_with_wait_code(exit_code);
                    unreachable!("exit_group_with_wait_code must not return");
                }
                task::block_current(task::TaskWaitTarget::Manual);
            }
            task::ProcessState::Exiting(exit_code) | task::ProcessState::Exited(exit_code) => {
                crate::sys::task::exit_current_with_wait_code(exit_code);
                unreachable!("exit_current_with_wait_code must not return");
            }
            task::ProcessState::Running => {}
        }
    }
    let snapshot = ensure_current_signal_state()?;
    let effect = ipc::signal::take_deliverable(snapshot.task_id);
    let Some(effect) = effect else {
        return Ok(false);
    };
    let pending = match effect {
        SignalEffect::Handler(pending) => pending,
        SignalEffect::Terminate { signal } => {
            let _ = take_delivery_signal_source(snapshot.task_id, snapshot.pid.raw(), signal);
            let exit_code =
                crate::sys::task::wait::signal_terminate_exit_code(signal, snapshot.task_id);
            crate::sys::task::exit_group_with_wait_code(exit_code);
            unreachable!("exit_group_with_wait_code must not return");
        }
        SignalEffect::Stop { signal } => {
            if task::mark_process_stopped(snapshot.pid, signal as u8).is_ok() {
                notify_parent_sigchld(snapshot.pid);
                task::wake_parent_child_waiters(snapshot.pid);
            }
            if let Some(task_ids) = task::task_ids_for_process(snapshot.pid) {
                for member in task_ids {
                    if member != snapshot.task_id {
                        task::request_task_reschedule(member);
                    }
                }
            }
            task::block_current(task::TaskWaitTarget::Manual);
            return Ok(false);
        }
        SignalEffect::Continue { .. } => return Ok(false),
    };

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
    let source = take_pending_signal_source(snapshot.task_id,
                                            snapshot.pid.raw(),
                                            pending.scope,
                                            pending.signal);
    let user_frame = build_user_signal_frame(user_siginfo(pending.signal, source),
                                                   signal_stack_for_user(alternate_stack,
                                                                         already_on_alternate),
                                                   pending.previous_mask
                                                          .bits(),
                                                   &original);
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
    if frame_sp & 0xF != 0 {
        return Err(ErrNo::EFAULT);
    }
    let user_frame = copy_from_user_struct::<UserRtSignalFrame>(frame_sp)?;
    let restored = restore_user_machine_context(&user_frame).ok_or(ErrNo::EFAULT)?;
    if !context.restore_signal_context(&restored) {
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
    let snapshot = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let task_id = snapshot.task_id;
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
    let wait = task::wait_queue::WaitQueue::new_named("sigsuspend");
    let _ =
        wait.wait_current_while(|| !ipc::signal::has_deliverable(snapshot.task_id).unwrap_or(true));
    let _ = wait.try_release_empty();
    if !ipc::signal::has_deliverable(snapshot.task_id).unwrap_or(false) {
        let _ = ipc::signal::end_sigsuspend(snapshot.task_id);
    }
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
    if set % core::mem::align_of::<u64>() != 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if sigset_size != RT_SIGSET_SIZE_64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let signal_snapshot = match ensure_current_signal_state() {
        Ok(snapshot) => snapshot,
        Err(error) => return UserRet::from_error(error),
    };
    let task_id = signal_snapshot.task_id;
    let process_pid = signal_snapshot.pid
                                     .raw();
    let wait_set = match copy_from_user_struct::<u64>(set) {
        Ok(bits) => SignalSet::from_bits(bits),
        Err(e) => return UserRet::from_error(e),
    };
    let deadline = if timeout == 0 {
        None
    } else {
        if timeout % core::mem::align_of::<UserTimespec>() != 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
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
    let wait_queue = task::wait_queue::WaitQueue::new_named("sigtimedwait");
    let pending = loop {
        if let Some(pending) = ipc::signal::take_pending_record(task_id, wait_set) {
            break pending;
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
            if let Some(pending) = ipc::signal::take_pending_record(task_id, wait_set) {
                break pending;
            }
            let _ = wait_queue.try_release_empty();
            return UserRet::from_error(ErrNo::EINTR);
        }
    };
    let _ = wait_queue.try_release_empty();
    let sig = pending.signal;
    let source = take_pending_signal_source(task_id, process_pid, pending.scope, sig);
    if info != 0 {
        let siginfo = user_siginfo(sig, source);
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
    if let Some(caller) = task::current_process_snapshot() {
        let uid = cred::current_credentials().effective_uid
                                             .0;
        record_pending_signal_source(PendingSignalOwner::Thread(task_id),
                                     signal,
                                     PendingSignalSource { pid : caller.pid.raw(),
                                                           uid,
                                                           ..PendingSignalSource::default() });
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
    if let Some(caller) = task::current_process_snapshot() {
        let uid = cred::current_credentials().effective_uid
                                             .0;
        record_pending_signal_source(PendingSignalOwner::Thread(task_id),
                                     signal,
                                     PendingSignalSource { pid : caller.pid.raw(),
                                                           uid,
                                                           ..PendingSignalSource::default() });
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

pub(crate) fn check_signal_permission(process : ProcessId, sig : usize) -> Result<(), ErrNo> {
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

pub(crate) fn send_signal_to_process(process : ProcessId, sig : usize) -> Result<(), ErrNo> {
    if task::leader_task_for_process(process).is_none() {
        return Err(ErrNo::ESRCH);
    }
    if task::process_snapshot(process).is_some_and(|snapshot| {
                                          matches!(snapshot.state,
                                                   task::ProcessState::Exited(_))
                                      })
    {
        return Ok(());
    }
    check_signal_permission(process, sig)?;
    if ensure_process_signal_state(process).is_err() {
        return Err(ErrNo::ESRCH);
    }
    if let Some(caller) = task::current_process_snapshot() {
        let uid = cred::current_credentials().effective_uid
                                             .0;
        record_pending_signal_source(PendingSignalOwner::Process(process.raw()),
                                     sig,
                                     PendingSignalSource { pid : caller.pid.raw(),
                                                           uid,
                                                           ..PendingSignalSource::default() });
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

/// Deliver a terminal-generated signal to every process in a foreground group.
///
/// This is a kernel-originated path: unlike `kill(2)` it deliberately bypasses
/// caller credential checks, while reusing the normal pending-signal,
/// stop/continue and scheduler wakeup machinery.
pub(crate) fn send_kernel_signal_to_process_group(pgid : ProcessId, sig : usize) -> usize {
    if sig == 0 || sig > NSIG {
        return 0;
    }
    let mut delivered = 0;
    for process in task::process_pids_in_pgid(pgid) {
        if ensure_process_signal_state(process).is_err() {
            continue;
        }
        let Ok(dispatch) = ipc::signal::send_process(process.raw(), sig) else {
            continue;
        };
        apply_signal_dispatch(dispatch, sig);
        if dispatch.delivery == SignalDelivery::Pending {
            if let Some(task_ids) = task::task_ids_for_process(process) {
                for member in task_ids {
                    if ipc::signal::has_deliverable(member).unwrap_or(false) {
                        let _ = task::interrupt_task(member);
                        task::request_task_reschedule(member);
                    }
                }
            }
        }
        delivered += 1;
    }
    delivered
}

/// 父进程退出时向直接子进程投递 `PR_SET_PDEATHSIG` 设置的信号。
///
/// 该路径由内核自动触发，不检查调用者权限；只投递给仍存活的子进程，避免向
/// 已 `Exiting/Exited` 的进程重复中断。
fn send_kernel_signal_to_process(process : ProcessId, sig : usize) -> Result<(), ErrNo> {
    if sig == 0 || sig > _NSIG as usize {
        return Err(ErrNo::EINVAL);
    }
    if task::leader_task_for_process(process).is_none() {
        return Err(ErrNo::ESRCH);
    }
    if task::process_snapshot(process).is_none_or(|snapshot| {
                                          matches!(snapshot.state,
                                                   task::ProcessState::Exited(_) |
                                                   task::ProcessState::Exiting(_))
                                      })
    {
        return Ok(());
    }
    if ensure_process_signal_state(process).is_err() {
        return Ok(());
    }
    let dispatch = ipc::signal::send_process(process.raw(), sig).map_err(|_| ErrNo::EINVAL)?;
    apply_signal_dispatch(dispatch, sig);
    if dispatch.delivery == SignalDelivery::Pending {
        if let Some(task_ids) = task::task_ids_for_process(process) {
            for member in task_ids {
                if ipc::signal::has_deliverable(member).unwrap_or(false) {
                    let _ = task::interrupt_task(member);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn deliver_parent_death_notifications(notifications : impl IntoIterator<Item = task::ParentDeathNotification>)
{
    for notification in notifications {
        if notification.signal > 0 {
            let _ = send_kernel_signal_to_process(notification.pid,
                                                  notification.signal as usize);
        }
    }
}

/// `kill(pid, sig)` — riscv64 系统调用号 129。
pub(crate) fn sys_kill(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let sig = args.arg(1) as i32;

    if sig < 0 || sig > _NSIG {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synchronous_fault_siginfo_uses_positive_code_and_address() {
        let address = 0x1234_5678usize;
        let info = user_siginfo(4,
                                PendingSignalSource { code : 1,
                                                      fault_addr : address,
                                                      ..PendingSignalSource::default() });
        assert_eq!(info.signo, 4);
        assert_eq!(info.code, 1);
        assert_eq!(&info.payload[..core::mem::size_of::<usize>()],
                   &address.to_ne_bytes());
    }

    #[test]
    fn user_signal_siginfo_keeps_pid_and_uid_layout() {
        let info = user_siginfo(15,
                                PendingSignalSource { pid : 42,
                                                      uid : 7,
                                                      ..PendingSignalSource::default() });
        assert_eq!(info.code, 0);
        assert_eq!(&info.payload[0..4], &42u32.to_ne_bytes());
        assert_eq!(&info.payload[4..8], &7u32.to_ne_bytes());
    }

    #[test]
    fn same_process_thread_fault_sources_do_not_overwrite_each_other() {
        let process_pid = usize::MAX - 10;
        let first_task = usize::MAX - 11;
        let second_task = usize::MAX - 12;
        let first_address = 0x1111_2222usize;
        let second_address = 0x3333_4444usize;

        record_pending_signal_source(PendingSignalOwner::Thread(first_task),
                                     ipc::signal::SIGSEGV,
                                     PendingSignalSource { code : 1,
                                                           fault_addr : first_address,
                                                           ..PendingSignalSource::default() });
        record_pending_signal_source(PendingSignalOwner::Thread(second_task),
                                     ipc::signal::SIGSEGV,
                                     PendingSignalSource { code : 1,
                                                           fault_addr : second_address,
                                                           ..PendingSignalSource::default() });

        let first = take_pending_signal_source(first_task,
                                               process_pid,
                                               PendingSignalScope::Thread,
                                               ipc::signal::SIGSEGV);
        let second = take_pending_signal_source(second_task,
                                                process_pid,
                                                PendingSignalScope::Thread,
                                                ipc::signal::SIGSEGV);
        assert_eq!(first.code, 1);
        assert_eq!(first.fault_addr, first_address);
        assert_eq!(second.code, 1);
        assert_eq!(second.fault_addr, second_address);
    }

    #[test]
    fn thread_and_process_signal_sources_have_distinct_keys() {
        let owner_id = usize::MAX - 20;
        let signal = ipc::signal::SIGILL;
        record_pending_signal_source(PendingSignalOwner::Thread(owner_id),
                                     signal,
                                     PendingSignalSource { code : 1,
                                                           fault_addr : 0x5555,
                                                           ..PendingSignalSource::default() });
        record_pending_signal_source(PendingSignalOwner::Process(owner_id),
                                     signal,
                                     PendingSignalSource { pid : 99,
                                                           uid : 7,
                                                           ..PendingSignalSource::default() });

        let thread = take_pending_signal_source(owner_id,
                                                owner_id,
                                                PendingSignalScope::Thread,
                                                signal);
        let process = take_pending_signal_source(owner_id,
                                                 owner_id,
                                                 PendingSignalScope::Process,
                                                 signal);
        assert_eq!(thread.code, 1);
        assert_eq!(thread.fault_addr, 0x5555);
        assert_eq!(process.pid, 99);
        assert_eq!(process.uid, 7);
    }
}
