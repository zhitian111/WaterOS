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
    Exception, Interrupt, SignalFrameCodec, SignalMachineContext, TrapCause, TrapFrameRead,
    TrapFrameWrite,
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
const CSR_EUEN : usize = 0x2;
const LOONGARCH_PAGE_SIZE_BITS : usize = 12;
const LOONGARCH_PWCL_4K_3LEVEL : usize =
    12 | (9 << 5) | (21 << 10) | (9 << 15);
const LOONGARCH_PWCH_4K_3LEVEL : usize = 30 | (9 << 6);
/// PLV0 专用直接映射窗口：VA[47:0] → PA[47:0]，MAT 为一致可缓存。
/// 此处不开放 PLV3，迫使用户代码走 PGDL/TLB，同时 trap/重填入口与内核栈不依赖
/// 当前用户 PGDL。
const LOONGARCH_DMW0_PLV0_CACHED : usize = 0x11;
/// `PRMD.PPLV`：返回后特权级域（与 `returns_to_user` 判定一致）。
const LOONGARCH_PRMD_PPLV_MASK : usize = 0x3;
/// `PRMD.PIE`：返回时全局中断使能快照位（与 `set_return_to_user_raw` 配合）。
const LOONGARCH_PRMD_PIE : usize = 1 << 2;
/// 用户态 PLV 编码（与手册中 PLV=3 对应；用于区分返回到用户还是内核）。
const LOONGARCH_USER_PLV : usize = 0x3;
/// `ESTAT.IS.TI`：定时器中断挂起位（与 `decode_loongarch64_trap_cause` 一致）。
const TIMER_INTERRUPT_PENDING : usize = 1 << 11;
/// `ESTAT.IS.IPI`：核间中断挂起位。
const IPI_INTERRUPT_PENDING : usize = 1 << 12;
/// 单次定时器中断后重新武装的切片长度（StableCounter
/// 刻度）；与调度策略相关，非用户 ABI。
const TIMER_SLICE_TICKS : u64 = 10_000_000;
/// `EUEN.FPE`：允许当前执行流使用基础浮点寄存器。
///
/// bring-up 阶段用户任务串行运行，先全局打开以支持 hard-float glibc/musl
/// busybox；后续多任务并发时应在任务切换路径补充 FPU 上下文保存/恢复。
const LOONGARCH_EUEN_FPE : usize = 1 << 0;

unsafe fn save_fp_state() -> ([u64; 32], u32) {
    let mut regs = [0u64; 32];
    let mut fcsr : usize;
    let base = regs.as_mut_ptr();
    unsafe {
        asm!(
            "fst.d $f0, {base}, 0", "fst.d $f1, {base}, 8",
            "fst.d $f2, {base}, 16", "fst.d $f3, {base}, 24",
            "fst.d $f4, {base}, 32", "fst.d $f5, {base}, 40",
            "fst.d $f6, {base}, 48", "fst.d $f7, {base}, 56",
            "fst.d $f8, {base}, 64", "fst.d $f9, {base}, 72",
            "fst.d $f10, {base}, 80", "fst.d $f11, {base}, 88",
            "fst.d $f12, {base}, 96", "fst.d $f13, {base}, 104",
            "fst.d $f14, {base}, 112", "fst.d $f15, {base}, 120",
            "fst.d $f16, {base}, 128", "fst.d $f17, {base}, 136",
            "fst.d $f18, {base}, 144", "fst.d $f19, {base}, 152",
            "fst.d $f20, {base}, 160", "fst.d $f21, {base}, 168",
            "fst.d $f22, {base}, 176", "fst.d $f23, {base}, 184",
            "fst.d $f24, {base}, 192", "fst.d $f25, {base}, 200",
            "fst.d $f26, {base}, 208", "fst.d $f27, {base}, 216",
            "fst.d $f28, {base}, 224", "fst.d $f29, {base}, 232",
            "fst.d $f30, {base}, 240", "fst.d $f31, {base}, 248",
            "movfcsr2gr {fcsr}, $fcsr0",
            base = in(reg) base,
            fcsr = out(reg) fcsr,
            options(nostack),
        );
    }
    (regs, fcsr as u32)
}

unsafe fn restore_fp_state(regs : &[u64; 32], fcsr : u32) {
    let base = regs.as_ptr();
    unsafe {
        asm!(
            "fld.d $f0, {base}, 0", "fld.d $f1, {base}, 8",
            "fld.d $f2, {base}, 16", "fld.d $f3, {base}, 24",
            "fld.d $f4, {base}, 32", "fld.d $f5, {base}, 40",
            "fld.d $f6, {base}, 48", "fld.d $f7, {base}, 56",
            "fld.d $f8, {base}, 64", "fld.d $f9, {base}, 72",
            "fld.d $f10, {base}, 80", "fld.d $f11, {base}, 88",
            "fld.d $f12, {base}, 96", "fld.d $f13, {base}, 104",
            "fld.d $f14, {base}, 112", "fld.d $f15, {base}, 120",
            "fld.d $f16, {base}, 128", "fld.d $f17, {base}, 136",
            "fld.d $f18, {base}, 144", "fld.d $f19, {base}, 152",
            "fld.d $f20, {base}, 160", "fld.d $f21, {base}, 168",
            "fld.d $f22, {base}, 176", "fld.d $f23, {base}, 184",
            "fld.d $f24, {base}, 192", "fld.d $f25, {base}, 200",
            "fld.d $f26, {base}, 208", "fld.d $f27, {base}, 216",
            "fld.d $f28, {base}, 224", "fld.d $f29, {base}, 232",
            "fld.d $f30, {base}, 240", "fld.d $f31, {base}, 248",
            "movgr2fcsr $fcsr0, {fcsr}",
            base = in(reg) base,
            fcsr = in(reg) fcsr as usize,
            options(nostack),
        );
    }
}

#[inline]
fn decode_loongarch64_trap_cause(estat : usize) -> TrapCause {
    if (estat & IPI_INTERRUPT_PENDING) != 0 {
        return TrapCause::Interrupt(Interrupt::SupervisiorSoft);
    }
    if (estat & TIMER_INTERRUPT_PENDING) != 0 {
        return TrapCause::Interrupt(Interrupt::SupervisiorTimer);
    }

    let ecode = (estat >> 16) & 0x3F;
    match ecode {
        1 | 7 | 8 => TrapCause::Exception(Exception::LoadPageFault),
        // ecode 2 = PIS (store invalid), ecode 4 = PME (page modified).
        2 | 4 => TrapCause::Exception(Exception::StorePageFault),
        3 | 6 => TrapCause::Exception(Exception::InstructionPageFault),
        9 => TrapCause::Exception(Exception::Breakpoint),
        11 => TrapCause::Exception(Exception::UserEnvCall),
        12 => TrapCause::Exception(Exception::Breakpoint),
        13 => TrapCause::Exception(Exception::IllegalInstruction),
        other => TrapCause::Exception(Exception::Unsupported(other)),
    }
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
    write_csr::<CSR_PWCH>(LOONGARCH_PWCH_4K_3LEVEL);
    write_csr::<CSR_ASID>(0);
    write_csr::<CSR_EUEN>(LOONGARCH_EUEN_FPE);
    unsafe {
        asm!("invtlb 0, $zero, $zero");
    }
}

/// LoongArch64 当前不需要 RISC-V `SUM` 一类的用户页访问准备。
#[inline]
pub fn prepare_user_trap_frame_access() {}

/// PLV0 通过 DMW0 直接访问内核 RAM/MMIO，用户 trap 处理期间可以保留用户
/// PGDL。这样普通 syscall 返回同一地址空间时无需往返切换 PGDL 和清空 TLB。
#[inline]
pub const fn user_trap_requires_kernel_address_space() -> bool { false }

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

    fn user_sp(&self) -> usize { self.x[3] }

    fn user_tls(&self) -> usize { self.x[2] }

    fn returns_to_user(&self) -> bool {
        (self.prmd & LOONGARCH_PRMD_PPLV_MASK) == LOONGARCH_USER_PLV
    }

    fn return_address_space_token(&self) -> usize { self.return_address_space_token }
    fn syscall_args(&self) -> SyscallArgs {
        SyscallArgs::from_regs([self.x[4], self.x[5], self.x[6], self.x[7], self.x[8], self.x[9]])
    }
    fn syscall_nr(&self) -> SyscallNumber { SyscallNumber(self.x[11]) }
}


impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc : usize) { self.era = pc; }

    fn add_user_pc(&mut self, bytes : usize) {
        self.era = self.era
                       .wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp : usize) { self.x[3] = sp; }

    fn set_user_entry_args(&mut self, _argc : usize, _argv : usize, _envp : usize) {
        // Linux/LoongArch libc 从用户栈读 argc/argv/envp；a0 供动态链接器 rtld_fini，
        // 内核直接 exec 的静态程序须置 0。
        self.x[4] = 0;
    }

    fn set_return_to_user(&mut self) {
        self.prmd = (self.prmd & !(LOONGARCH_PRMD_PPLV_MASK | LOONGARCH_PRMD_PIE)) |
                    LOONGARCH_USER_PLV |
                    LOONGARCH_PRMD_PIE;
    }

    fn set_return_to_kernel(&mut self) { self.prmd &= !LOONGARCH_PRMD_PPLV_MASK; }
    fn set_return_address_space_token(&mut self, token : usize) {
        self.return_address_space_token = token;
    }
    fn set_syscall_ret(&mut self, ret : UserRet) { self.x[4] = ret.0 as usize; }
    fn set_user_tls(&mut self, tls : usize) {
        // LoongArch64 psABI：线程指针为 $r2。
        self.x[2] = tls;
    }
}


impl SignalFrameCodec for TrapContext {
    fn capture_signal_context(&self) -> SignalMachineContext {
        let (fpregs, fcsr) = unsafe { save_fp_state() };
        SignalMachineContext { gprs : self.x,
                               pc : self.era,
                               status : self.prmd,
                               fpregs,
                               fcsr,
                               reserved : 0 }
    }

    fn restore_signal_context(&mut self, context : &SignalMachineContext) -> bool {
        if context.pc == 0 || context.pc & 3 != 0 {
            return false;
        }
        self.x = context.gprs;
        self.x[0] = 0;
        self.era = context.pc;
        unsafe {
            restore_fp_state(&context.fpregs, context.fcsr);
        }
        self.set_return_to_user();
        true
    }

    fn prepare_signal_handler(&mut self,
                              handler : usize,
                              restorer : usize,
                              frame_sp : usize,
                              signal : usize,
                              siginfo : usize,
                              ucontext : usize) {
        self.x[1] = restorer;
        self.x[3] = frame_sp;
        self.x[4] = signal;
        self.x[5] = siginfo;
        self.x[6] = ucontext;
        self.era = handler;
        self.set_return_to_user();
    }

    fn prepare_syscall_restart(context : &mut SignalMachineContext,
                               syscall_nr : usize,
                               args : [usize; 6],
                               instruction_bytes : usize) {
        context.pc = context.pc
                            .wrapping_sub(instruction_bytes);
        context.gprs[4..10].copy_from_slice(&args);
        context.gprs[11] = syscall_nr;
    }
}
