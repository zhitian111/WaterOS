//! RISC-V S 模式 **trap 帧与最小入口**：与 `asm/trap.asm` 中 `__alltraps` / [`trap_entry_rust`] 对齐。
//!
//! **路由**：业务在组合层经 [`wateros_platform_arch_api_v0::kernel_trap`] 注册；本文件只做帧类型与
//! **一次** `invoke_kernel_trap_handler`，不依赖 `task`/`syscall`。

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::kernel_trap;
use api_v0::trap::{
    TrapFrameRead, TrapFrameWrite, TrapSyscallRead, TrapSyscallWrite,
};
use core::arch::asm;

/// 该结构的字段顺序/大小必须与 `asm/trap.asm` 的偏移严格一致（方案A）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapContext {
    pub(crate) x: [usize; 32],
    pub(crate) sstatus: usize,
    pub(crate) sepc: usize,
    pub(crate) scause: usize,
    pub(crate) stval: usize,
}

const RISCV_SSTATUS_SPP: usize = 1 << 8;

impl TrapContext {
    #[inline]
    pub const fn raw_cause(&self) -> usize {
        self.scause
    }

    #[inline]
    pub const fn user_pc(&self) -> usize {
        self.sepc
    }

    #[inline]
    pub const fn user_sp(&self) -> usize {
        self.x[2]
    }

    #[inline]
    pub const fn fault_addr(&self) -> usize {
        self.stval
    }

    #[inline]
    pub const fn returns_to_user(&self) -> bool {
        (self.sstatus & RISCV_SSTATUS_SPP) == 0
    }

    #[inline]
    pub const fn returns_to_kernel(&self) -> bool {
        !self.returns_to_user()
    }

    #[inline]
    fn syscall_nr_raw(&self) -> usize {
        self.x[17]
    }

    #[inline]
    fn syscall_args_raw(&self) -> SyscallArgs {
        SyscallArgs::from_regs([
            self.x[10],
            self.x[11],
            self.x[12],
            self.x[13],
            self.x[14],
            self.x[15],
        ])
    }

    #[inline]
    fn user_sp_raw(&self) -> usize {
        self.x[2]
    }

    #[inline]
    fn set_user_sp_raw(&mut self, sp: usize) {
        self.x[2] = sp;
    }

    #[inline]
    fn returns_to_user_raw(&self) -> bool {
        (self.sstatus & RISCV_SSTATUS_SPP) == 0
    }

    #[inline]
    fn set_return_to_user_raw(&mut self) {
        self.sstatus &= !RISCV_SSTATUS_SPP;
        self.sstatus &= !(1 << 1);
        self.sstatus |= 1 << 5;
    }

    #[inline]
    fn set_return_to_kernel_raw(&mut self) {
        self.sstatus |= RISCV_SSTATUS_SPP;
    }
}

unsafe extern "C" {
    fn __alltraps();
}

<<<<<<< HEAD
=======
const TIMER_SLICE_TICKS: u64 = 1_250_000;
static TIMER_TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
const SYSCALL_INSN_BYTES: usize = 4;

#[inline]
fn decode_riscv64_trap_cause(scause: usize) -> TrapCause {
    let is_interrupt = (scause >> (usize::BITS - 1)) != 0;
    let code = scause & 0xFFF;

    if is_interrupt {
        match code {
            1 => TrapCause::Interrupt(Interrupt::SupervisiorSoft),
            5 => TrapCause::Interrupt(Interrupt::SupervisiorTimer),
            9 => TrapCause::Interrupt(Interrupt::SupervisiorExternel),
            other => TrapCause::Interrupt(Interrupt::Unsupported(other)),
        }
    } else {
        match code {
            8 => TrapCause::Exception(Exception::UserEnvCall),
            12 => TrapCause::Exception(Exception::InstructionPageFault),
            13 => TrapCause::Exception(Exception::LoadPageFault),
            15 => TrapCause::Exception(Exception::StorePageFault),
            2 => TrapCause::Exception(Exception::IllegalInstruction),
            3 => TrapCause::Exception(Exception::Breakpoint),
            other => TrapCause::Exception(Exception::Unsupported(other)),
        }
    }
}

>>>>>>> github/main
/// 初始化 trap 入口：把 `stvec` 指向 `__alltraps`。
pub fn init_trap() {
    let addr = __alltraps as *const () as usize;
    let stvec = addr & !0x3;
    unsafe {
        asm!("csrw stvec, {0}", in(reg) stvec);
    }
}

/// 汇编入口转入：交给组合层 [`kernel_trap::invoke_kernel_trap_handler`]。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr: *mut TrapContext) {
<<<<<<< HEAD
    kernel_trap::invoke_kernel_trap_handler(cx_ptr.cast());
=======
    let authoritative_cx_ptr =
        unsafe { __wateros_task_runtime_begin_current_trap_frame_access(cx_ptr.cast::<u8>()) };
    let cx = unsafe { &mut *(authoritative_cx_ptr as *mut TrapContext) };

    let trap_cause = decode_riscv64_trap_cause(cx.scause);
    match trap_cause {
        TrapCause::Exception(Exception::UserEnvCall) => {
            handle_user_syscall(cx);
        }
        TrapCause::Exception(Exception::InstructionPageFault)
        | TrapCause::Exception(Exception::LoadPageFault)
        | TrapCause::Exception(Exception::StorePageFault) => {
            logging::debug!(
                "[trap] page fault: cause={:?} scause={:#x?} sepc={:#x?} stval={:#x?}",
                trap_cause,
                cx.scause,
                cx.sepc,
                cx.stval
            );
        }
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            let now = super::time::Riscv64ArchTime::read_time_tick()
                .expect("read time tick during trap")
                .0;
            let deadline = now.saturating_add(TIMER_SLICE_TICKS);
            if let Err(err) = firmware::timer::set_timer(FirmwareTimerDeadline(deadline)) {
                panic!(
                    "failed to re-arm timer in trap: {:?}",
                    err
                );
            }
            let tick = TIMER_TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if tick % 8 == 0 {
                logging::trace!("[trap] timer tick {}", tick);
            }
            unsafe {
                __wateros_task_runtime_schedule_tick();
            }
        }
        _ => {
            panic!(
                "unexpected trap: cause={:?}, sepc={:#x}, stval={:#x}",
                trap_cause, cx.sepc, cx.stval
            );
        }
    }

    if cx.returns_to_user() {
        logging::trace!(
            "[trap] return to user pc={:#x} sp={:#x}",
            cx.user_pc(),
            cx.user_sp()
        );
    }

    unsafe {
        __wateros_task_runtime_restore_current_trap_frame(cx_ptr.cast::<u8>());
    }
}

fn handle_user_syscall(cx: &mut TrapContext) {
    let syscall_nr = cx
        .syscall_nr()
        .raw();
    let syscall_args = cx.syscall_args();
    let syscall_ret = unsafe {
        __wateros_syscall_dispatch_current(
            syscall_nr,
            syscall_args.arg(0),
            syscall_args.arg(1),
            syscall_args.arg(2),
            syscall_args.arg(3),
            syscall_args.arg(4),
            syscall_args.arg(5),
        )
    };
    cx.add_user_pc(SYSCALL_INSN_BYTES);
    cx.set_syscall_ret(UserRet(syscall_ret));
>>>>>>> github/main
}

impl TrapFrameRead for TrapContext {
    fn raw_cause(&self) -> usize {
        self.scause
    }

    fn trap_cause(&self) -> TrapCause {
        decode_riscv64_trap_cause(self.scause)
    }

    fn fault_addr(&self) -> usize {
        self.stval
    }

    fn user_pc(&self) -> usize {
        self.sepc
    }

    fn user_sp(&self) -> usize {
        self.user_sp_raw()
    }

    fn returns_to_user(&self) -> bool {
        self.returns_to_user_raw()
    }
}

impl TrapSyscallRead for TrapContext {
    fn syscall_args(&self) -> SyscallArgs {
        self.syscall_args_raw()
    }

    fn syscall_nr(&self) -> SyscallNumber {
        SyscallNumber(self.syscall_nr_raw())
    }
}

impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc: usize) {
        self.sepc = pc;
    }

    fn add_user_pc(&mut self, bytes: usize) {
        self.sepc = self.sepc.wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp: usize) {
        self.set_user_sp_raw(sp);
    }

    fn set_return_to_user(&mut self) {
        self.set_return_to_user_raw();
    }

    fn set_return_to_kernel(&mut self) {
        self.set_return_to_kernel_raw();
    }
}

impl TrapSyscallWrite for TrapContext {
    fn set_syscall_ret(&mut self, ret: UserRet) {
        self.x[10] = ret.0 as usize;
    }
}
