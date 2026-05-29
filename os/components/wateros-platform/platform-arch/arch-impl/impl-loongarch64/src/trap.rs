//! LoongArch64 **异常入口与帧语义**：与 `asm/trap.S` 中 `__alltraps`
//! 保存顺序一致； `TrapContext` 字段与 CSR
//! 槽位（`prmd`/`era`/`estat`/`badv`）变更时必须同步汇编。
//!
//! **ABI**：用户态系统调用按 LoongArch Linux 约定使用 `$r4`–`r9` 为参数、`$r11`
//! 为 系统调用号；返回值写入 `$r4`（见 `TrapSyscallWrite`）。

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::kernel_trap;
use api_v0::trap::{
    Exception, Interrupt, TrapAddressSpaceWrite, TrapCause, TrapFrameRead, TrapFrameWrite,
    TrapSyscallRead, TrapSyscallWrite, TrapThreadWrite,
};
use core::arch::asm;

/// 字段顺序/大小必须与 `asm/trap.S` 的偏移保持一致。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapContext {
    x : [usize; 32],
    prmd : usize,
    era : usize,
    estat : usize,
    badv : usize,
    return_address_space_token : usize,
}

/// 异常入口向量 CSR（`EENTRY`）：`trap.S` 中 `__alltraps` 的物理入口地址写入此
/// CSR。
const CSR_EENTRY : usize = 0xC;
const CSR_ASID : usize = 0x18;
const CSR_PWCL : usize = 0x1C;
const CSR_PWCH : usize = 0x1D;
const CSR_STLBPS : usize = 0x1E;
const CSR_TLBRENTRY : usize = 0x88;
const CSR_TLBREHI : usize = 0x8E;
const CSR_DMW0 : usize = 0x180;
const LOONGARCH_PAGE_SIZE_BITS : usize = 12;
const LOONGARCH_PWCL_4K_3LEVEL : usize =
    12 | (9 << 5) | (21 << 10) | (9 << 15) | (30 << 20) | (9 << 25);
/// PLV0-only direct-map window for VA[47:0] -> PA[47:0], MAT=coherent cached.
/// Keeping PLV3 disabled here forces user code through PGDL/TLB while making
/// trap/refill entry and kernel stacks independent of the current user PGDL.
const LOONGARCH_DMW0_PLV0_CACHED : usize = 0x11;
/// `PRMD.PPLV`：返回后特权级域（与 `returns_to_user` 判定一致）。
const LOONGARCH_PRMD_PPLV_MASK : usize = 0x3;
/// `PRMD.PIE`：返回时全局中断使能快照位（与 `set_return_to_user_raw` 配合）。
const LOONGARCH_PRMD_PIE : usize = 1 << 2;
/// 用户态 PLV 编码（与手册中 PLV=3 对应；用于区分返回到用户还是内核）。
const LOONGARCH_USER_PLV : usize = 0x3;
/// `ESTAT.IS.TI`：定时器中断挂起位（与 `decode_loongarch64_trap_cause` 一致）。
const TIMER_INTERRUPT_PENDING : usize = 1 << 11;
/// 单次定时器中断后重新武装的切片长度（StableCounter
/// 刻度）；与调度策略相关，非用户 ABI。
const TIMER_SLICE_TICKS : u64 = 10_000_000;

#[inline]
fn decode_loongarch64_trap_cause(estat : usize) -> TrapCause {
    if (estat & TIMER_INTERRUPT_PENDING) != 0 {
        return TrapCause::Interrupt(Interrupt::SupervisiorTimer);
    }

    let ecode = (estat >> 16) & 0x3F;
    match ecode {
        1 | 2 | 7 | 8 => TrapCause::Exception(Exception::LoadPageFault),
        // ecode 8 = PPI (Page Privilege Illegal)
        3 | 6 => TrapCause::Exception(Exception::InstructionPageFault),
        4 => TrapCause::Exception(Exception::StorePageFault),
        9 => TrapCause::Exception(Exception::Breakpoint),
        11 => TrapCause::Exception(Exception::UserEnvCall),
        12 => TrapCause::Exception(Exception::Breakpoint),
        13 => TrapCause::Exception(Exception::IllegalInstruction),
        other => TrapCause::Exception(Exception::Unsupported(other)),
    }
}

impl TrapContext {
    #[inline]
    fn syscall_nr_raw(&self) -> usize {
        // LoongArch64 Linux ABI: a7($r11) 保存 syscall id。
        self.x[11]
    }

    #[inline]
    fn syscall_args_raw(&self) -> SyscallArgs {
        // LoongArch64 Linux ABI: a0..a5 依次是 $r4..$r9。
        SyscallArgs::from_regs([self.x[4], self.x[5], self.x[6], self.x[7], self.x[8], self.x[9]])
    }

    #[inline]
    fn user_sp_raw(&self) -> usize { self.x[3] }

    #[inline]
    fn set_user_sp_raw(&mut self, sp : usize) { self.x[3] = sp; }

    #[inline]
    fn returns_to_user_raw(&self) -> bool {
        (self.prmd & LOONGARCH_PRMD_PPLV_MASK) == LOONGARCH_USER_PLV
    }

    #[inline]
    fn set_return_to_user_raw(&mut self) {
        self.prmd = (self.prmd & !(LOONGARCH_PRMD_PPLV_MASK | LOONGARCH_PRMD_PIE)) |
                    LOONGARCH_USER_PLV |
                    LOONGARCH_PRMD_PIE;
    }

    #[inline]
    fn set_return_to_kernel_raw(&mut self) { self.prmd &= !LOONGARCH_PRMD_PPLV_MASK; }
}

unsafe extern "C" {
    fn __alltraps();
    fn __tlb_refill();
}

/// `csrwr`：写 CSR 并返回旧值；此处丢弃旧值，仅作副作用写。
#[inline]
fn write_csr<const CSR: usize>(value : usize) {
    let old = value;
    unsafe {
        asm!("csrwr {0}, {1}", inout(reg) old => _, const CSR);
    }
}

/// 安装异常入口：将 `__alltraps` 写入 `EENTRY`（与 `trap.S` 中符号地址一致）。
pub fn init_trap() {
    let addr = __alltraps as *const () as usize;
    write_csr::<CSR_DMW0>(LOONGARCH_DMW0_PLV0_CACHED);
    write_csr::<CSR_EENTRY>(addr);
    write_csr::<CSR_TLBRENTRY>(__tlb_refill as *const () as usize);
    write_csr::<CSR_STLBPS>(LOONGARCH_PAGE_SIZE_BITS);
    write_csr::<CSR_TLBREHI>(LOONGARCH_PAGE_SIZE_BITS);
    write_csr::<CSR_PWCL>(LOONGARCH_PWCL_4K_3LEVEL);
    write_csr::<CSR_PWCH>(0);
    write_csr::<CSR_ASID>(0);
    unsafe {
        asm!("invtlb 0, $zero, $zero");
    }
}

/// LoongArch64 当前不需要 RISC-V `SUM` 一类的用户页访问准备。
#[inline]
pub fn prepare_user_trap_frame_access() {}

/// 当前 LoongArch64 trap 调度时间片长度。
#[inline]
pub const fn timer_slice_ticks() -> u64 { TIMER_SLICE_TICKS }

/// 汇编 `bl trap_entry_rust` 转入：在权威 trap 帧指针上完成 syscall / 定时器 /
/// 其它异常的组合层分发；本层只负责保持帧布局和解码语义。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr : *mut TrapContext) {
    kernel_trap::invoke_kernel_trap_handler(cx_ptr.cast());
}

impl TrapFrameRead for TrapContext {
    fn raw_cause(&self) -> usize { self.estat }

    fn trap_cause(&self) -> TrapCause { decode_loongarch64_trap_cause(self.estat) }

    fn fault_addr(&self) -> usize { self.badv }

    fn user_pc(&self) -> usize { self.era }

    fn user_sp(&self) -> usize { self.user_sp_raw() }

    fn returns_to_user(&self) -> bool { self.returns_to_user_raw() }
}

impl TrapSyscallRead for TrapContext {
    fn syscall_args(&self) -> SyscallArgs { self.syscall_args_raw() }

    fn syscall_nr(&self) -> SyscallNumber { SyscallNumber(self.syscall_nr_raw()) }
}

impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc : usize) { self.era = pc; }

    fn add_user_pc(&mut self, bytes : usize) {
        self.era = self.era
                       .wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp : usize) { self.set_user_sp_raw(sp); }

    fn set_return_to_user(&mut self) { self.set_return_to_user_raw(); }

    fn set_return_to_kernel(&mut self) { self.set_return_to_kernel_raw(); }
}

impl TrapAddressSpaceWrite for TrapContext {
    fn set_return_address_space_token(&mut self, token : usize) {
        self.return_address_space_token = token;
    }
}

impl TrapSyscallWrite for TrapContext {
    fn set_syscall_ret(&mut self, ret : UserRet) { self.x[4] = ret.0 as usize; }
}

impl TrapThreadWrite for TrapContext {
    fn set_user_tls(&mut self, tls : usize) {
        // LoongArch64 psABI: tp is $r2.
        self.x[2] = tls;
    }
}
