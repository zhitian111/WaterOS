//! 仅供 `gdb-fault-injection` 测试构建使用的确定性故障。
//!
//! GDB 写 `WATEROS_DEBUG_FAULT_MODE` 后，下一次 timer trap 执行故障。普通内核
//! 不编译本模块，不能被误触发。

use core::sync::atomic::{AtomicUsize, Ordering};

/// 0=off, 1=fixed loop, 2=two-CPU ABBA, 3=stop local timer,
/// 4=keep timer but suppress scheduling。
#[unsafe(no_mangle)]
pub static WATEROS_DEBUG_FAULT_MODE : AtomicUsize = AtomicUsize::new(0);

static ABBA_ARRIVED : AtomicUsize = AtomicUsize::new(0);

fn current_cpu() -> usize { platform::arch::cpu::current_cpu_id().raw() }

static ABBA_A : debug::TrackedMutex<()> = debug::TrackedMutex::new((),
                                                                   debug::DebugLockKind::Scheduler,
                                                                   current_cpu);
static ABBA_B : debug::TrackedMutex<()> =
    debug::TrackedMutex::new((),
                             debug::DebugLockKind::ProcessRegistry,
                             current_cpu);

/// 返回 true 表示本次 timer trap 仍可返回，但必须跳过 scheduler tick。
pub fn on_timer(cpu : usize) -> bool {
    let mode = WATEROS_DEBUG_FAULT_MODE.load(Ordering::Acquire);
    if mode != 0 {
        debug::update_cpu_state(cpu, |state| {
            state.last_schedule_reason = debug::DEBUG_FAULT_REASON_BASE | mode as u32;
        });
    }
    match mode {
        0 => false,
        1 if cpu == 0 => loop {
            core::hint::spin_loop();
        },
        2 if cpu < 2 => {
            let (first, second) = if cpu == 0 {
                (&ABBA_A, &ABBA_B)
            } else {
                (&ABBA_B, &ABBA_A)
            };
            let _first = first.lock();
            ABBA_ARRIVED.fetch_or(1usize << cpu, Ordering::AcqRel);
            while ABBA_ARRIVED.load(Ordering::Acquire) & 0b11 != 0b11 {
                core::hint::spin_loop();
            }
            // 两个 CPU 都持有 first 后再申请对方的锁，稳定形成 ABBA。
            let _second = second.lock();
            unreachable!("ABBA fault unexpectedly acquired both locks");
        }
        2 => false,
        3 if cpu == 0 => {
            let _ = platform::arch::interrupt::disable_timer_interrupt();
            true
        }
        4 if cpu == 0 => true,
        _ => false,
    }
}
