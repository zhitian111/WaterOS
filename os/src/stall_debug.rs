//! 低频系统调用停滞诊断。
//!
//! 热路径只更新原子量；独立内核任务定期采样。连续无系统调用进展时，
//! 才复制并输出任务状态和可能相关的 futex 等待链。不要在 syscall/timer
//! 热路径直接打印：并行编译会产生大量 `mprotect`/`munmap`，同步串口日志
//! 本身足以严重扰动 SMP 运行。

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

const MAX_CPUS : usize = base_config::task::MAX_CPUS;
const SAMPLE_INTERVAL_TICKS : u64 = 100;
const REPORT_AFTER_SAMPLES : usize = 5;
const REPORT_INTERVAL_SAMPLES : usize = 10;
const FUTEX_SYSCALL_NR : usize = 98;
const BRK_SYSCALL_NR : usize = 214;
const MUNMAP_SYSCALL_NR : usize = 215;
const MPROTECT_SYSCALL_NR : usize = 226;

static STARTED : AtomicBool = AtomicBool::new(false);
static SYSCALL_TOTAL : AtomicU64 = AtomicU64::new(0);
static LAST_SYSCALL_NR : AtomicUsize = AtomicUsize::new(0);
static TIMER_ENTRIES : [AtomicU64; MAX_CPUS] = [const { AtomicU64::new(0) }; MAX_CPUS];
/// 0 表示当前没有受跟踪的内存布局 syscall，其余值为 syscall nr。
static MEMORY_SYSCALL_NR : [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static MEMORY_SYSCALL_PC : [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static MEMORY_SYSCALL_ARG0 : [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static MEMORY_SYSCALL_ARG1 : [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];
static MEMORY_SYSCALL_ARG2 : [AtomicUsize; MAX_CPUS] = [const { AtomicUsize::new(0) }; MAX_CPUS];

/// 记录 syscall 入口。返回值是供 [`record_syscall_exit`] 使用的 opaque token；
/// 0 表示该调用不需要清理 per-CPU 诊断状态。
#[inline]
pub fn record_syscall_enter(syscall_nr : usize, args : [usize; 6], user_pc : usize) -> usize {
    LAST_SYSCALL_NR.store(syscall_nr, Ordering::Relaxed);
    SYSCALL_TOTAL.fetch_add(1, Ordering::Relaxed);
    let cpu = platform::arch::cpu::current_cpu_id().raw();
    debug::update_cpu_state(cpu, |state| {
        state.syscalls = state.syscalls.wrapping_add(1);
        state.last_syscall_nr = syscall_nr as u64;
        state.last_syscall_pc = user_pc as u64;
    });
    debug::record_event(cpu,
                        task::current_tick(),
                        task::current_task_id().map_or(debug::NO_TASK, |id| id as u64),
                        debug::DebugEventKind::SyscallEnter,
                        0,
                        [syscall_nr as u64, user_pc as u64, args[0] as u64]);
    if cpu >= MAX_CPUS {
        return 0;
    }
    if !traces_memory_syscall(syscall_nr) {
        return (syscall_nr << 8) | cpu.saturating_add(1);
    }
    // nr 最后以 Release 发布，watchdog 以 Acquire 读取后才访问其余字段。
    MEMORY_SYSCALL_PC[cpu].store(user_pc, Ordering::Relaxed);
    MEMORY_SYSCALL_ARG0[cpu].store(args[0], Ordering::Relaxed);
    MEMORY_SYSCALL_ARG1[cpu].store(args[1], Ordering::Relaxed);
    MEMORY_SYSCALL_ARG2[cpu].store(args[2], Ordering::Relaxed);
    MEMORY_SYSCALL_NR[cpu].store(syscall_nr, Ordering::Release);
    (syscall_nr << 8) | cpu.saturating_add(1)
}

#[inline]
pub fn record_syscall_exit(token : usize) {
    let Some(cpu) = (token & 0xff).checked_sub(1) else {
        return;
    };
    if cpu < MAX_CPUS {
        let syscall_nr = token >> 8;
        debug::record_event(cpu,
                            task::current_tick(),
                            task::current_task_id().map_or(debug::NO_TASK, |id| id as u64),
                            debug::DebugEventKind::SyscallExit,
                            0,
                            [syscall_nr as u64, 0, 0]);
        MEMORY_SYSCALL_NR[cpu].store(0, Ordering::Release);
    }
}

#[inline]
fn traces_memory_syscall(syscall_nr : usize) -> bool {
    matches!(syscall_nr,
             BRK_SYSCALL_NR | MUNMAP_SYSCALL_NR | MPROTECT_SYSCALL_NR)
}

/// 记录硬件 timer 已到达该 CPU。这里只观察中断是否持续到达，不尝试在
/// `schedule_tick()` 后配对：发生上下文切换时，调用点之后的代码会等原任务
/// 再次运行才继续，不能代表调度器临界区的退出时刻。
pub fn record_timer(cpu : usize) {
    if cpu >= MAX_CPUS {
        return;
    }
    let count = TIMER_ENTRIES[cpu].fetch_add(1, Ordering::Relaxed) + 1;
    debug::update_cpu_state(cpu, |state| {
        state.timer_ticks = count;
    });
    if count & 63 == 0 {
        debug::record_event(cpu,
                            task::current_tick(),
                            task::current_task_id().map_or(debug::NO_TASK, |id| id as u64),
                            debug::DebugEventKind::Timer,
                            0,
                            [count, 0, 0]);
    }
}

fn log_execution_snapshot() {
    for cpu in 0..MAX_CPUS {
        let timer_count = TIMER_ENTRIES[cpu].load(Ordering::Acquire);
        let syscall_nr = MEMORY_SYSCALL_NR[cpu].load(Ordering::Acquire);
        if syscall_nr == 0 {
            runtime::logging::warn!("[stall-debug][cpu] cpu={} timers={} memory-syscall=none",
                                    cpu,
                                    timer_count);
            continue;
        }
        runtime::logging::warn!("[stall-debug][cpu] cpu={} timers={} memory-syscall={} \
                                 pc={:#x} args=[{:#x},{:#x},{:#x}]",
                                cpu,
                                timer_count,
                                syscall_nr,
                                MEMORY_SYSCALL_PC[cpu].load(Ordering::Relaxed),
                                MEMORY_SYSCALL_ARG0[cpu].load(Ordering::Relaxed),
                                MEMORY_SYSCALL_ARG1[cpu].load(Ordering::Relaxed),
                                MEMORY_SYSCALL_ARG2[cpu].load(Ordering::Relaxed));
    }
}

pub fn syscall_snapshot() -> (u64, usize) {
    (SYSCALL_TOTAL.load(Ordering::Relaxed), LAST_SYSCALL_NR.load(Ordering::Relaxed))
}

/// 启动唯一的低频停滞看门狗任务。
pub fn start() {
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    task::spawn_kernel_task(stall_watchdog_task, 0);
}

extern "C" fn stall_watchdog_task(_arg : usize) -> ! {
    let mut previous_total = SYSCALL_TOTAL.load(Ordering::Relaxed);
    let mut stalled_samples = 0usize;

    loop {
        task::sleep_for_ticks(SAMPLE_INTERVAL_TICKS);

        let (total, last_syscall) = syscall_snapshot();
        if total == 0 || total != previous_total {
            previous_total = total;
            stalled_samples = 0;
            continue;
        }

        stalled_samples = stalled_samples.saturating_add(1);
        let should_report =
            stalled_samples == REPORT_AFTER_SAMPLES ||
            (stalled_samples > REPORT_AFTER_SAMPLES &&
             (stalled_samples - REPORT_AFTER_SAMPLES) % REPORT_INTERVAL_SAMPLES == 0);
        if !should_report {
            continue;
        }

        let stalled_ticks = (stalled_samples as u64).saturating_mul(SAMPLE_INTERVAL_TICKS);
        runtime::logging::warn!("[stall-debug] no syscall progress for {} ticks total={} last={} \
                                 (0x{:x})",
                                stalled_ticks,
                                total,
                                last_syscall,
                                last_syscall);
        log_execution_snapshot();
        task::log_stall_diagnostics();
        if last_syscall == FUTEX_SYSCALL_NR {
            ipc::futex::log_debug_snapshot();
        }
    }
}
