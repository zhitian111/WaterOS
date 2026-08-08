//! **组合层内核 trap 路由**：实现各架构 `trap_entry_rust` 交出的 `TrapCause`
//! 分支与返回路径，
//! 经 [`arch_api_v0::kernel_trap::register_kernel_trap_handler`] 挂接到
//! `trap_entry_rust`。
//!
//! **须在** `task::init()` **之后**调用 [`init`]。

use arch_api_v0::kernel_trap::register_kernel_trap_handler;
use arch_api_v0::trap::{Exception, Interrupt, TrapCause, TrapFrameRead, TrapFrameWrite};
use base_config::task::SCHED_TIMER_PERIOD_MS;
use core::sync::atomic::{AtomicUsize, Ordering};
use mm::api::mmap::PageFaultAccess;
use platform::arch::paging;
use platform::arch::trap::ActiveTrapFrame as TrapContext;
use runtime::logging::*;
use syscall::dispatch_syscall_from_trap;
use syscall::UserRet;
use syscall::{EXEC, RT_SIGRETURN};

#[inline]
fn record_debug_trap(kind : debug::DebugEventKind, cx : &TrapContext, raw_cause : usize) {
    if !debug::ENABLED {
        return;
    }
    let cpu = platform::arch::cpu::current_cpu_id().raw();
    let task_id = task::current_task_id().map_or(debug::NO_TASK, |id| id as u64);
    let tick = task::current_tick();
    if matches!(kind, debug::DebugEventKind::TrapEnter) {
        debug::update_cpu_state(cpu, |state| {
            state.traps = state.traps
                               .wrapping_add(1);
            state.last_trap_cause = raw_cause as u64;
            state.last_trap_pc = cx.user_pc() as u64;
            state.last_trap_sp = cx.user_sp() as u64;
            state.last_fault_addr = cx.fault_addr() as u64;
        });
    }
    debug::record_event(cpu, tick, task_id, kind, 0, [raw_cause
                                                      as u64,
                                                      cx.user_pc()
                                                      as u64,
                                                      cx.fault_addr()
                                                      as u64]);
}

/// 热路径 syscall/trap 跟踪；release 构建默认关闭。
macro_rules! hot_syscall_trace {
    ($($tt:tt)*) => {
        #[cfg(any(debug_assertions, feature = "syscall-trace"))]
        { trace!($($tt)*); }
    };
}

#[inline]
fn exit_current_if_process_exiting() {
    if let Some(process) = task::current_process_snapshot() {
        if let task::ProcessState::Exiting(exit_code) = process.state {
            task::exit_group_current(exit_code);
        }
    }
}

/// 监督态定时器中断后，用 **与 `kernel_main` 相同的 wall-clock 语义**
/// 重新武装固件定时器。
///
/// 先前使用 `read_time_tick() + TIMER_SLICE_TICKS` 的 **裸 `time` CSR
/// 增量**；在部分 QEMU/频率 组合下若与 SBI `set_timer`
/// 期望的刻度或固件内部换算不一致，可能导致 **deadline 落在「现在」之前**
/// 或极近，从而 **S 态定时器中断连发**。此时 GDB `SIGINT` 常在
/// `trampoline_start`（`__alltraps`） 处采样到 PC，看起来像「卡在
/// trampoline」，实为 **trap 风暴**而非 `sret` 损坏。
const TIMER_REARM_MS : u64 = SCHED_TIMER_PERIOD_MS;
static TIMER_TICK_COUNT : AtomicUsize = AtomicUsize::new(0);
/// 当前支持架构的 syscall/trap 指令宽度，用于将用户 PC 前进到下一条指令。
const SYSCALL_INSN_BYTES : usize = 4;

/// 在投递 SIGSEGV 前打印任务与用户态 fault 上下文。
fn log_unhandled_user_fault_probe(cx : &TrapContext, trap_cause : TrapCause, raw_cause : usize) {
    if let Some(s) = task::current_process_task_snapshot() {
        debug!("[trap][probe] proc pid={} tid={} task_id={} role={:?}",
               s.pid.raw(),
               s.tid.raw(),
               s.task_id,
               s.role);
    } else {
        debug!("[trap][probe] proc (no current process task)");
    }
    if let Some(s) = task::current_task_snapshot() {
        debug!("[trap][probe] task parent={:?} state={:?} kind={:?}",
               s.parent_id, s.state, s.kind);
    }
    debug!("[trap][probe] fault cause={:?} raw={:#x} ecode={:#x} sepc={:#x} stval={:#x} sp={:#x} \
            tp={:#x} satp={:#x} aspace={:#x}",
           trap_cause,
           raw_cause,
           (raw_cause >> 16) & 0x3F,
           cx.user_pc(),
           cx.fault_addr(),
           cx.user_sp(),
           cx.user_tls(),
           cx.return_address_space_token(),
           task::current_task_user_aspace_ptr());
}

/// 记录用户任务 trap 杀进程上下文并终止当前进程。
fn kill_current_user_task(context : &str, trap_cause : TrapCause, cx : &TrapContext) -> ! {
    if let Some(snapshot) = task::current_task_snapshot() {
        if snapshot.kind != task::TaskKind::User {
            if !cx.returns_to_user() || task::current_process_task_snapshot().is_none() {
                fatal_kernel_trap("attempted to terminate a non-user task",
                                  trap_cause,
                                  cx.raw_cause(),
                                  cx);
            }
            warn!("[trap] user trap on mismatched task kind={:?} task_id={} state={:?} \
                   returns_to_user=true; terminating current process",
                  snapshot.kind, snapshot.id, snapshot.state);
        }
        warn!("[trap] killing user task ({}) cause={:?} pc={:#x} fault_addr={:#x} task_id={} \
               parent_id={:?} state={:?}",
              context,
              trap_cause,
              cx.user_pc(),
              cx.fault_addr(),
              snapshot.id,
              snapshot.parent_id,
              snapshot.state);
    } else {
        warn!("[trap] killing user task ({}) cause={:?} pc={:#x} fault_addr={:#x} (no current \
               task snapshot)",
              context,
              trap_cause,
              cx.user_pc(),
              cx.fault_addr());
    }
    syscall::terminate_current_process(-1);
}

/// 内核态不可恢复 trap：记录诊断后停机，避免 `sret` 到损坏 PC 形成级联 fault。
fn fatal_kernel_trap(context : &str,
                     trap_cause : TrapCause,
                     raw_cause : usize,
                     cx : &TrapContext)
                     -> ! {
    error!("[trap] fatal kernel trap ({}) cause={:?} raw_cause={:#x} pc={:#x} fault_addr={:#x} \
            returns_to_user={}",
           context,
           trap_cause,
           raw_cause,
           cx.user_pc(),
           cx.fault_addr(),
           cx.returns_to_user());
    loop {
        let _ = platform::arch::interrupt::wait_for_interrupt();
    }
}

/// 组合层内核 trap 入口：由 `arch` 在异常/中断向量中调用，`frame` 为当前 trap
/// 帧字节指针。
///
/// 处理 `UserEnvCall`（分派 syscall）、页错日志、监督态定时器 tick（重武装 +
/// 调度），其余 cause 直接 `panic`。返回用户态前由 arch
/// 层完成必要的帧访问准备。
extern "C" fn wateros_kernel_trap_handler(frame : *mut u8) {
    let stack_cx = unsafe { &mut *(frame as *mut TrapContext) };
    let authoritative = if stack_cx.returns_to_user() {
        unsafe { task::begin_current_trap_frame_access(frame) }
    } else {
        frame
    };
    let cx = unsafe { &mut *(authoritative as *mut TrapContext) };

    if cx.returns_to_user() {
        exit_current_if_process_exiting();
        if platform::arch::trap::user_trap_requires_kernel_address_space() {
            let kernel_satp = mm::kernel_mm::kernel_satp();
            if paging::active_address_space_token() != kernel_satp {
                paging::activate_address_space_token_and_flush(kernel_satp);
            }
        }
        platform::arch::trap::prepare_user_trap_frame_access();
    }
    let raw_cause = cx.raw_cause();
    record_debug_trap(debug::DebugEventKind::TrapEnter,
                      cx,
                      raw_cause);
    let trap_cause = cx.trap_cause();
    let mut restart = None;
    match trap_cause {
        TrapCause::Exception(Exception::UserEnvCall) => {
            let syscall_nr = cx.syscall_nr()
                               .raw();
            let syscall_args = cx.syscall_args();
            #[cfg_attr(not(any(debug_assertions, feature = "syscall-trace")),
                       allow(unused_variables))]
            let regs = syscall_args.as_regs();
            #[cfg(feature = "stall-debug")]
            let stall_trace =
                crate::stall_debug::record_syscall_enter(syscall_nr, regs, cx.user_pc());
            hot_syscall_trace!("[syscall] nr={} user_pc={:#x} user_sp={:#x} \
                                args=[{:#x},{:#x},{:#x},{:#x},{:#x},{:#x}]",
                               syscall_nr,
                               cx.user_pc(),
                               cx.user_sp(),
                               regs[0],
                               regs[1],
                               regs[2],
                               regs[3],
                               regs[4],
                               regs[5],);
            if syscall_nr == RT_SIGRETURN {
                if !syscall::restore_signal_frame(authoritative) {
                    let has_user_context = task::current_task_snapshot().is_some_and(|task| {
                                                                            task.kind ==
                                                                            task::TaskKind::User
                                                                        }) &&
                                           task::current_process_task_snapshot().is_some();
                    if has_user_context {
                        kill_current_user_task("invalid rt_sigreturn frame",
                                               trap_cause,
                                               cx);
                    }
                    warn!("[trap] ignoring invalid rt_sigreturn on non-user context pc={:#x}",
                          cx.user_pc());
                    cx.add_user_pc(SYSCALL_INSN_BYTES);
                    cx.set_syscall_ret(UserRet(syscall::ErrNo::EINVAL.user_ret()));
                    finish_trap_return(frame, cx, raw_cause);
                    return;
                }
                hot_syscall_trace!("[syscall] nr={} restored signal frame",
                                   syscall_nr);
                return_to_user_signal_delivery(authoritative, trap_cause, cx, None);
                finish_trap_return(frame, cx, raw_cause);
                return;
            }
            let syscall_ret = dispatch_syscall_from_trap(syscall_nr, syscall_args);
            #[cfg(feature = "stall-debug")]
            crate::stall_debug::record_syscall_exit(stall_trace);
            hot_syscall_trace!("[syscall] nr={} ret={}",
                               syscall_nr,
                               syscall_ret);
            // execve 成功时已替换整个 trap 帧，跳过 sepc 推进与返回值写入；
            // 失败时必须像普通 syscall 一样把 -errno 返回给原用户态，否则会反复执行同一条 ecall。
            let exec_succeeded = syscall_nr == EXEC && syscall_ret >= 0;
            if !exec_succeeded {
                cx.add_user_pc(SYSCALL_INSN_BYTES);
                cx.set_syscall_ret(UserRet(syscall_ret));
                if syscall_ret == syscall::ErrNo::EINTR.user_ret() &&
                   syscall::is_restartable_syscall(syscall_nr)
                {
                    restart = Some((syscall_nr, syscall_args));
                }
            }
        }
        TrapCause::Exception(Exception::InstructionPageFault) |
        TrapCause::Exception(Exception::LoadPageFault) |
        TrapCause::Exception(Exception::StorePageFault) => {
            // 先前仅 `debug!` 并继续 `sret`：若 fault 发生在用户态，会回到同一 sepc 立即再
            // fault， 形成无限 trap 风暴；在 INFO
            // 日志级别下毫无输出，表现为「sret 后卡死」。
            if cx.returns_to_user() {
                if matches!(trap_cause,
                            TrapCause::Exception(Exception::StorePageFault)) &&
                   mm::kernel_mm::handle_cow_fault(task::current_task_user_aspace_ptr(),
                                                   cx.fault_addr())
                {
                    trace!("[trap] handled user COW fault sepc={:#x} stval={:#x}",
                           cx.user_pc(),
                           cx.fault_addr());
                    finish_trap_return(frame, cx, raw_cause);
                    return;
                }
                let fault_access = match trap_cause {
                    TrapCause::Exception(Exception::InstructionPageFault) => {
                        PageFaultAccess::Execute
                    }
                    TrapCause::Exception(Exception::LoadPageFault) => PageFaultAccess::Read,
                    TrapCause::Exception(Exception::StorePageFault) => PageFaultAccess::Write,
                    _ => unreachable!(),
                };
                if mm::kernel_mm::handle_user_page_fault(task::current_task_user_aspace_ptr(),
                                                         cx.fault_addr(),
                                                         fault_access)
                {
                    syscall::record_user_page_fault_handled();
                    trace!("[trap] handled user lazy page fault sepc={:#x} stval={:#x}",
                           cx.user_pc(),
                           cx.fault_addr());
                    finish_trap_return(frame, cx, raw_cause);
                    return;
                }
                log_unhandled_user_fault_probe(cx, trap_cause, raw_cause);
                debug!("[trap] user memory fault {:?} raw={:#x} ecode={:#x} sepc={:#x} \
                        stval={:#x} user_sp={:#x} return_satp={:#x} aspace_ptr={:#x} — \
                        delivering SIGSEGV",
                       trap_cause,
                       raw_cause,
                       (raw_cause >> 16) & 0x3F,
                       cx.user_pc(),
                       cx.fault_addr(),
                       cx.user_sp(),
                       cx.return_address_space_token(),
                       task::current_task_user_aspace_ptr());
                let raised = syscall::raise_current_signal(11);
                debug!("[trap][probe] raise_current_signal(SIGSEGV) -> {}",
                       raised);
                if !raised {
                    kill_current_user_task("user memory fault", trap_cause, cx);
                }
                let delivered = return_to_user_signal_delivery(authoritative, trap_cause, cx, None);
                if !delivered {
                    error!("[trap] SIGSEGV signal not delivered — killing user task");
                    kill_current_user_task("user memory fault (no signal delivered)",
                                           trap_cause,
                                           cx);
                }
                finish_trap_return(frame, cx, raw_cause);
                return;
            }
            fatal_kernel_trap("kernel page fault",
                              trap_cause,
                              raw_cause,
                              cx);
        }
        TrapCause::Interrupt(Interrupt::SupervisiorSoft) => {
            // 来自其他 CPU 的 IPI：清除本地 pending 位，避免 trap 返回后立即重入。
            platform::smp::clear_ipi();
            let pending = platform::smp::take_pending_ipi(platform::arch::cpu::current_cpu_id());
            if debug::ENABLED {
                let cpu = platform::arch::cpu::current_cpu_id().raw();
                debug::update_cpu_state(cpu, |state| {
                    state.ipi_received = state.ipi_received
                                              .wrapping_add(1);
                });
                debug::record_event(cpu,
                                    task::current_tick(),
                                    task::current_task_id().map_or(debug::NO_TASK, |id| id as u64),
                                    debug::DebugEventKind::IpiReceive,
                                    0,
                                    [pending as u64,
                                     0,
                                     0]);
            }
            if pending & platform::smp::IpiKind::TlbShootdown.bits() != 0 {
                let _ = mm::kernel_mm::handle_tlb_shootdown_ipi();
            }
            if pending & platform::smp::IpiKind::TaskNotify.bits() != 0 {
                if cx.returns_to_user() {
                    return_to_user_signal_delivery(authoritative, trap_cause, cx, None);
                }
                task::schedule_reschedule();
            } else if pending & platform::smp::IpiKind::Reschedule.bits() != 0 {
                // IPI is not a timer event: never advance timeout accounting or
                // consume a timeslice here.
                task::schedule_reschedule();
            }
        }
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            if let Err(err) = platform::timer::set_timer_after_ms(TIMER_REARM_MS) {
                panic!("failed to re-arm timer in trap: {:?}",
                       err);
            }
            #[cfg(feature = "stall-debug")]
            crate::stall_debug::record_timer(platform::arch::cpu::current_cpu_id().raw());
            #[cfg(feature = "gdb-fault-injection")]
            let suppress_scheduler =
                crate::debug_fault::on_timer(platform::arch::cpu::current_cpu_id().raw());
            #[cfg(not(feature = "gdb-fault-injection"))]
            let suppress_scheduler = false;
            let tick = TIMER_TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            #[cfg(feature = "bringup-stats")]
            if tick % 300 == 0 {
                syscall::log_thread_bringup_stats_summary();
            }
            if tick % 8 == 0 {
                trace!("[trap] timer tick {}", tick);
            }
            if !cx.returns_to_user() {
                exit_current_if_process_exiting();
            }
            syscall::timer_tick(cx.returns_to_user());
            if !suppress_scheduler {
                task::schedule_tick();
            }
        }
        _ => {
            if cx.returns_to_user() {
                let signal = match trap_cause {
                    TrapCause::Exception(Exception::IllegalInstruction) => 4,
                    _ => 11,
                };
                warn!("[trap] unhandled user exception cause={:?} raw={:#x} pc={:#x} \
                       fault_addr={:#x} signal={}",
                      trap_cause,
                      raw_cause,
                      cx.user_pc(),
                      cx.fault_addr(),
                      signal);
                if !syscall::raise_current_signal(signal) {
                    kill_current_user_task("user exception", trap_cause, cx);
                }
                return_to_user_signal_delivery(authoritative, trap_cause, cx, None);
                finish_trap_return(frame, cx, raw_cause);
                return;
            }
            fatal_kernel_trap("unexpected trap",
                              trap_cause,
                              raw_cause,
                              cx);
        }
    }

    if cx.returns_to_user() {
        exit_current_if_process_exiting();
        return_to_user_signal_delivery(authoritative, trap_cause, cx, restart);
        // `raw_cause` 来自 TrapContext.scause 快照，即 **本次** 进入内核的原因（如
        // ecall=0x8）， 不是硬件 CSR 的“下一异常预告”；`sret`
        // 前也不会用该槽位预测下一次 trap。
        hot_syscall_trace!("[trap] sret to user pc={:#x} sp={:#x} return_satp={:#x} \
                            kernel_satp={:#x} frame_scause={:#x} (this trap's scause snapshot)",
                           cx.user_pc(),
                           cx.user_sp(),
                           cx.return_address_space_token(),
                           paging::active_address_space_token(),
                           raw_cause,);
    }

    record_debug_trap(debug::DebugEventKind::TrapExit,
                      cx,
                      raw_cause);

    // --- 返回路径（与 `trap.asm` 成对）：本函数返回后 **没有** 更多 Rust
    // 代码会执行；汇编从 `sp` 指向的 `TrapContext` 装载 CSR/通用寄存器并
    // `sret`。因此不会出现「已成功回到用户态」的 INFO 日志——除非
    // 用户态再次 trap 或 syscall。
    //
    // 调用链摘要：
    // 1. `begin_current_trap_frame_access(frame)`：把内核栈上的快照写入当前 TCB 的
    //    `trap_frame`，并返回 TCB
    //    内权威缓冲区的指针（`cx`）；若尚无当前任务则仍用栈上 `frame`。
    // 2. 若来自用户：入口已切到内核地址空间（syscall/FS 等在内核 token 下执行）。
    // 3. 业务分支（如 syscall）在 `cx` 上改 `sepc`/`a0`/…。
    // 4. `restore_current_trap_frame(frame)`：把 TCB 内已更新的 `TrapContext`
    //    **拷回** `trap.asm` 传入的
    //    `frame`（内核栈上的那份），并按返回目标写入地址空间 token，供下面汇编
    //    `ld`/`csrw`/`sret` 使用。`false` 且回用户则 panic。
    let restored = if cx.returns_to_user() {
        unsafe { task::restore_current_trap_frame(frame) }
    } else {
        true
    };
    hot_syscall_trace!("[trap] restore_current_trap_frame restored={}",
                       restored);
    if cx.returns_to_user() && !restored {
        // 诊断：打印当前任务与断点 PC，区分“current 缓存与硬件脱节（真双核/迁移
        // 残留）”与“trap 处理中途切走了当前任务”。sepc 为内核地址且 current 为
        // 内核任务 → 更可能是 sscratch 误分类；sepc 为用户地址且 current 非用户
        // → 真脱节（current 缓存与硬件不一致）。
        let current = task::current_task_snapshot();
        panic!("restore_current_trap_frame failed before sret to user (current task trap_frame \
                missing? cpu={} current_id={:?} kind={:?} state={:?} sepc={:#x})",
               platform::arch::cpu::current_cpu_id().raw(),
               current.as_ref()
                      .map(|s| s.id),
               current.as_ref()
                      .map(|s| s.kind),
               current.as_ref()
                      .map(|s| s.state),
               cx.user_pc());
    }
}

/// 向待返回用户态的任务投递挂起信号；`restart` 供 EINTR 后重入 syscall 使用。
fn return_to_user_signal_delivery(frame : *mut u8,
                                  trap_cause : TrapCause,
                                  cx : &TrapContext,
                                  restart : Option<(usize, syscall::SyscallArgs)>)
                                  -> bool {
    let delivered = syscall::deliver_pending_signal(frame, restart);
    if delivered < 0 {
        let has_user_context =
            task::current_task_snapshot().is_some_and(|task| task.kind == task::TaskKind::User) &&
            task::current_process_task_snapshot().is_some();
        if has_user_context {
            kill_current_user_task("signal frame setup failed",
                                   trap_cause,
                                   cx);
        }
        warn!("[trap] ignoring signal frame setup failure on non-user context pc={:#x}",
              cx.user_pc());
        return false;
    }
    delivered > 0
}

/// 信号/页错等提前返回路径：打 trace 后把 TCB trap 帧拷回内核栈供 `sret`。
fn finish_trap_return(frame : *mut u8, cx : &TrapContext, raw_cause : usize) {
    if cx.returns_to_user() {
        exit_current_if_process_exiting();
    }
    hot_syscall_trace!("[trap] sret to user pc={:#x} sp={:#x} return_satp={:#x} \
                        kernel_satp={:#x} frame_scause={:#x}",
                       cx.user_pc(),
                       cx.user_sp(),
                       cx.return_address_space_token(),
                       paging::active_address_space_token(),
                       raw_cause);
    record_debug_trap(debug::DebugEventKind::TrapExit,
                      cx,
                      raw_cause);
    let restored = unsafe { task::restore_current_trap_frame(frame) };
    if !restored {
        panic!("restore_current_trap_frame failed before signal return");
    }
}

/// 向 `arch_api_v0` 注册本模块的 C ABI trap 回调；须在 `task::init()`
/// 之后调用。
pub fn init() { register_kernel_trap_handler(wateros_kernel_trap_handler); }
