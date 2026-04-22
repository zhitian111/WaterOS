use abi::syscall_args::{SyscallArgs, SyscallPacket};
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum Exception {
    UserEnvCall,
    InstructionPageFault,
    LoadPageFault,
    StorePageFault,
    IllegalInstruction,
    Breakpoint,
    Unsupported(usize),
}
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum Interrupt {
    SupervisiorTimer,
    SupervisiorExternel,
    SupervisiorSoft,
    Unsupported(usize),
}
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum TrapCause {
    Exception(Exception),
    Interrupt(Interrupt),
}

/// 从原始 trap cause 编码解码为 `TrapCause`。
///
/// 说明：这里的 `usize` **约定为 RISC-V 的 `scause` CSR 编码**（最高位表示
/// interrupt）。 如果后续支持其它架构，应当把该 `From<usize>` 实现迁移到对应的
/// arch-impl 层， 或改为 `From<ArchRawCause>` 的形式避免歧义。
impl From<usize> for TrapCause {
    #[inline]
    fn from(scause : usize) -> Self {
        let is_interrupt = (scause >> 63) != 0;
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
}

impl TrapCause {
    #[inline]
    #[allow(unused)]
    pub fn is_exception(&self) -> bool {
        match self {
            TrapCause::Exception(_) => true,
            TrapCause::Interrupt(_) => false,
        }
    }
    #[inline]
    #[allow(unused)]
    pub fn is_interrupt(&self) -> bool {
        match self {
            TrapCause::Exception(_) => false,
            TrapCause::Interrupt(_) => true,
        }
    }
    #[inline]
    #[allow(unused)]
    pub fn as_exception(self) -> Option<Exception> {
        if self.is_exception() {
            if let TrapCause::Exception(exception) = self {
                Some(exception)
            } else {
                None
            }
        } else {
            None
        }
    }
    #[inline]
    #[allow(unused)]
    pub fn as_interrupt(self) -> Option<Interrupt> {
        if self.is_interrupt() {
            if let TrapCause::Interrupt(interrupt) = self {
                Some(interrupt)
            } else {
                None
            }
        } else {
            None
        }
    }
}

#[allow(unused)]
pub trait TrapContextRead {
    fn trap_cause(&self) -> TrapCause;
    fn fault_addr(&self) -> usize;
    fn user_pc(&self) -> usize;
    fn user_sp(&self) -> usize;
    fn returns_to_user(&self) -> bool;
    fn syscall_args(&self) -> SyscallArgs;
    fn syscall_nr(&self) -> SyscallNumber;
    #[inline]
    #[allow(unused)]
    fn returns_to_kernel(&self) -> bool { !self.returns_to_user() }
    #[inline]
    #[allow(unused)]
    fn syscall_context(&self) -> SyscallPacket {
        SyscallPacket::new(self.syscall_nr(), self.syscall_args())
    }
}

#[allow(unused)]
pub trait TrapCOntextWrite {
    fn set_syscall_ret(&mut self, ret : UserRet);
    fn set_user_pc(&mut self, pc : usize);
    fn add_user_pc(&mut self, bytes : usize);
    fn set_user_sp(&mut self, sp : usize);
    fn set_return_to_user(&mut self);
    fn set_return_to_kernel(&mut self);
    #[inline]
    #[allow(unused)]
    fn prepare_user_return(&mut self, entry_pc : usize, user_sp : usize) {
        self.set_user_pc(entry_pc);
        self.set_user_sp(user_sp);
        self.set_return_to_user();
    }
}

#[allow(unused)]
pub trait TrapContextFrameView: TrapContextRead + TrapCOntextWrite {}
