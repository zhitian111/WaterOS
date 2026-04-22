use abi::syscall_args::SyscallArgs;
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::time::ArchTime;
use api_v0::trap::{Interrupt, TrapCOntextWrite, TrapCause, TrapContextRead};
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};
use firmware::timer::FirmwareTimerDeadline;

unsafe extern "C" {
    fn __wateros_schedule_tick();
}

/// 该结构的字段顺序/大小必须与 `asm/trap.asm` 的偏移严格一致（方案A）。
#[repr(C)]
pub struct TrapContext {
    x : [usize; 32],
    sstatus : usize,
    sepc : usize,
    scause : usize,
    stval : usize,
}

impl TrapContext {
    #[inline]
    fn syscall_nr_raw(&self) -> usize {
        // Linux/riscv64：a7(x17) 保存 syscall id
        self.x[17]
    }

    #[inline]
    fn syscall_args_raw(&self) -> SyscallArgs {
        // Linux/riscv64：a0..a5 依次是 x10..x15
        SyscallArgs::from_regs([self.x[10], self.x[11], self.x[12], self.x[13], self.x[14],
                                self.x[15]])
    }
}

unsafe extern "C" {
    fn __alltraps();
}

const TIMER_SLICE_TICKS: u64 = 1_250_000;
static TIMER_TICK_COUNT: AtomicUsize = AtomicUsize::new(0);

/// 初始化 trap 入口：把 `stvec` 指向 `__alltraps`。
///
/// 注意：更完整的实现还需要初始化 page table / trap context / 中断使能。
pub fn init_trap() {
    let addr = __alltraps as *const () as usize;
    let stvec = addr & !0x3; // direct 模式
    unsafe {
        asm!("csrw stvec, {0}", in(reg) stvec);
    }
}

/// 方案A：入口汇编只保存上下文到栈上，然后跳转到该 Rust 入口。
/// 第一阶段在内核态任务之间切换后，最终回到 trap 汇编完成恢复与 `sret`。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr : *mut TrapContext) {
    let cx = unsafe { &mut *cx_ptr };

    let trap_cause = TrapCause::from(cx.scause);
    match trap_cause {
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            let now = super::time::Riscv64ArchTime::read_time_tick()
                .expect("read time tick during trap")
                .0;
            let deadline = now.saturating_add(TIMER_SLICE_TICKS);
            if let Err(err) = firmware::timer::set_timer(FirmwareTimerDeadline(deadline)) {
                panic!("failed to re-arm timer in trap: {:?}", err);
            }
            let tick = TIMER_TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if tick % 8 == 0 {
                logging::trace!("[trap] timer tick {}", tick);
            }
            unsafe {
                __wateros_schedule_tick();
            }
        }
        _ => {
            panic!(
                "unexpected trap: cause={:?}, sepc={:#x}, stval={:#x}",
                trap_cause,
                cx.sepc,
                cx.stval
            );
        }
    }
}

impl TrapContextRead for TrapContext {
    fn trap_cause(&self) -> TrapCause { TrapCause::from(self.scause) }

    fn fault_addr(&self) -> usize {
        // page fault 时，stval 是 fault address（其他异常可忽略）
        self.stval
    }

    fn user_pc(&self) -> usize { self.sepc }

    fn syscall_args(&self) -> SyscallArgs { self.syscall_args_raw() }

    fn syscall_nr(&self) -> SyscallNumber { SyscallNumber(self.syscall_nr_raw()) }
}

impl TrapCOntextWrite for TrapContext {
    fn set_syscall_ret(&mut self, ret : UserRet) {
        // Linux/riscv64：返回值在 a0(x10)
        self.x[10] = ret.0 as usize;
    }

    fn set_user_pc(&mut self, pc : usize) { self.sepc = pc; }

    fn add_user_pc(&mut self, bytes : usize) {
        self.sepc = self.sepc
                        .wrapping_add(bytes);
    }
}
