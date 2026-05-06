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

impl TrapCause {
    #[inline]
    #[allow(unused)]
    pub fn is_exception(&self) -> bool {
        matches!(self, TrapCause::Exception(_))
    }

    #[inline]
    #[allow(unused)]
    pub fn is_interrupt(&self) -> bool {
        matches!(self, TrapCause::Interrupt(_))
    }

    #[inline]
    #[allow(unused)]
    pub fn as_exception(self) -> Option<Exception> {
        match self {
            TrapCause::Exception(exception) => Some(exception),
            TrapCause::Interrupt(_) => None,
        }
    }

    #[inline]
    #[allow(unused)]
    pub fn as_interrupt(self) -> Option<Interrupt> {
        match self {
            TrapCause::Exception(_) => None,
            TrapCause::Interrupt(interrupt) => Some(interrupt),
        }
    }
}

#[allow(unused)]
pub trait TrapFrameRead {
    fn raw_cause(&self) -> usize;
    fn trap_cause(&self) -> TrapCause;
    fn fault_addr(&self) -> usize;
    fn user_pc(&self) -> usize;
    fn user_sp(&self) -> usize;
    fn returns_to_user(&self) -> bool;

    #[inline]
    #[allow(unused)]
    fn returns_to_kernel(&self) -> bool {
        !self.returns_to_user()
    }
}

#[allow(unused)]
pub trait TrapFrameWrite {
    fn set_user_pc(&mut self, pc: usize);
    fn add_user_pc(&mut self, bytes: usize);
    fn set_user_sp(&mut self, sp: usize);
    fn set_return_to_user(&mut self);
    fn set_return_to_kernel(&mut self);

    #[inline]
    #[allow(unused)]
    fn prepare_user_return(&mut self, entry_pc: usize, user_sp: usize) {
        self.set_user_pc(entry_pc);
        self.set_user_sp(user_sp);
        self.set_return_to_user();
    }
}

#[allow(unused)]
pub trait TrapSyscallRead {
    fn syscall_args(&self) -> SyscallArgs;
    fn syscall_nr(&self) -> SyscallNumber;

    #[inline]
    #[allow(unused)]
    fn syscall_context(&self) -> SyscallPacket {
        SyscallPacket::new(self.syscall_nr(), self.syscall_args())
    }
}

#[allow(unused)]
pub trait TrapSyscallWrite {
    fn set_syscall_ret(&mut self, ret: UserRet);
}

/// Read-only semantic view of an architecture trap context.
///
/// This is the compatibility facade used by older task code. New
/// architecture implementations should implement `TrapFrameRead` and
/// `TrapSyscallRead`; this trait is provided automatically.
#[allow(unused)]
pub trait TrapContextRead: TrapFrameRead + TrapSyscallRead {
    #[inline]
    fn raw_cause(&self) -> usize {
        TrapFrameRead::raw_cause(self)
    }

    #[inline]
    fn trap_cause(&self) -> TrapCause {
        TrapFrameRead::trap_cause(self)
    }

    #[inline]
    fn fault_addr(&self) -> usize {
        TrapFrameRead::fault_addr(self)
    }

    #[inline]
    fn user_pc(&self) -> usize {
        TrapFrameRead::user_pc(self)
    }

    #[inline]
    fn user_sp(&self) -> usize {
        TrapFrameRead::user_sp(self)
    }

    #[inline]
    fn returns_to_user(&self) -> bool {
        TrapFrameRead::returns_to_user(self)
    }

    #[inline]
    fn returns_to_kernel(&self) -> bool {
        TrapFrameRead::returns_to_kernel(self)
    }

    #[inline]
    fn syscall_args(&self) -> SyscallArgs {
        TrapSyscallRead::syscall_args(self)
    }

    #[inline]
    fn syscall_nr(&self) -> SyscallNumber {
        TrapSyscallRead::syscall_nr(self)
    }

    #[inline]
    fn syscall_context(&self) -> SyscallPacket {
        TrapSyscallRead::syscall_context(self)
    }
}

impl<T: TrapFrameRead + TrapSyscallRead + ?Sized> TrapContextRead for T {}

/// Mutable semantic view of an architecture trap context.
///
/// This compatibility facade joins frame-control writes with syscall-result
/// writes while keeping the implementor side split by responsibility.
#[allow(unused)]
pub trait TrapContextWrite: TrapFrameWrite + TrapSyscallWrite {
    #[inline]
    fn set_syscall_ret(&mut self, ret: UserRet) {
        TrapSyscallWrite::set_syscall_ret(self, ret);
    }

    #[inline]
    fn set_user_pc(&mut self, pc: usize) {
        TrapFrameWrite::set_user_pc(self, pc);
    }

    #[inline]
    fn add_user_pc(&mut self, bytes: usize) {
        TrapFrameWrite::add_user_pc(self, bytes);
    }

    #[inline]
    fn set_user_sp(&mut self, sp: usize) {
        TrapFrameWrite::set_user_sp(self, sp);
    }

    #[inline]
    fn set_return_to_user(&mut self) {
        TrapFrameWrite::set_return_to_user(self);
    }

    #[inline]
    fn set_return_to_kernel(&mut self) {
        TrapFrameWrite::set_return_to_kernel(self);
    }

    #[inline]
    #[allow(unused)]
    fn prepare_user_return(&mut self, entry_pc: usize, user_sp: usize) {
        TrapFrameWrite::prepare_user_return(self, entry_pc, user_sp);
    }
}

impl<T: TrapFrameWrite + TrapSyscallWrite + ?Sized> TrapContextWrite for T {}

#[allow(unused)]
pub trait TrapFrame: TrapContextRead + TrapContextWrite {}

impl<T: TrapContextRead + TrapContextWrite + ?Sized> TrapFrame for T {}

/// Backward-compatible alias for the earlier misspelled trait name.
#[allow(unused)]
#[deprecated(note = "use TrapContextWrite instead")]
pub trait TrapCOntextWrite: TrapContextWrite {}

#[allow(deprecated)]
impl<T: TrapContextWrite + ?Sized> TrapCOntextWrite for T {}

#[allow(unused)]
#[deprecated(note = "use TrapFrame instead")]
pub trait TrapContextFrameView: TrapContextRead + TrapContextWrite {}

#[allow(deprecated)]
impl<T: TrapContextRead + TrapContextWrite + ?Sized> TrapContextFrameView for T {}

/// 架构层可被任务系统保存、恢复和语义读取的 trap frame。
///
/// 具体寄存器布局由当前架构实现决定；task 只依赖这里的语义契约，
/// 不再在自身 API 中复制一份架构相关 frame 布局。
pub trait ArchTrapFrame:
    TrapContextRead + TrapContextWrite + Clone + Copy + core::fmt::Debug + Default + PartialEq + Eq
{
}

impl<T> ArchTrapFrame for T where
    T: TrapContextRead
        + TrapContextWrite
        + Clone
        + Copy
        + core::fmt::Debug
        + Default
        + PartialEq
        + Eq
{
}
