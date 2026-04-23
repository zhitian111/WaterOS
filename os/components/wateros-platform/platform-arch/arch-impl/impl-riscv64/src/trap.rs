use abi::errno::ErrNo;
use abi::syscall_args::{SyscallArgs, SyscallPacket};
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::trap::{Exception, Interrupt, TrapCOntextWrite, TrapCause, TrapContextRead};

use core::arch::asm;

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

/// 未来你会在这里接入 abi 内的 syscall 处理函数。
/// 现在先返回 ENOSYS，确保接口链条能跑通（你接入后再删除这个 stub）。
#[inline(never)]
fn abi_syscall_entry(_packet : SyscallPacket) -> UserRet { UserRet::from_error(ErrNo::ENOSYS) }

unsafe extern "C" {
    fn __alltraps();
}

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
/// 当前阶段只用于可观测测试；处理完不期望返回。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr : *mut TrapContext) -> ! {
    let cx = unsafe { &mut *cx_ptr };

    // 为了避免中断嵌套把栈/上下文覆盖，需要先关掉 timer 中断与全局中断。
    unsafe {
        use riscv::register::{sie, sstatus};
        sie::clear_stimer();
        sstatus::clear_sie();
    }

    let trap_cause = TrapCause::from(cx.scause);
    logging::debug!("[trap] trap_cause : {:?}", trap_cause);
    match trap_cause {
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            logging::debug!("[trap] timer interrupt: scause={:#x?} sepc={:#x?}",
                            cx.scause,
                            cx.sepc);
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
        TrapCause::Exception(Exception::UserEnvCall) => {
            // syscall 入口（目前仍用 stub，后续你会接入 abi 跳转）
            let packet = cx.syscall_context();
            let ret = abi_syscall_entry(packet);
            cx.set_syscall_ret(ret);
            cx.add_user_pc(4);
            logging::debug!("[trap] ecall (stub): scause={:#x?}",
                            cx.scause);
        }
        _ => {
            logging::debug!("[trap] other: scause={:#x?} sepc={:#x?}",
                            cx.scause,
                            cx.sepc);
        }
    }

    loop {
        core::hint::spin_loop();
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
