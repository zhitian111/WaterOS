//! **组合层内核 trap 路由**：实现各架构 `trap_entry_rust` 交出的 `TrapCause`
//! 分支与返回路径，
//! 经 [`arch_api_v0::kernel_trap::register_kernel_trap_handler`] 挂接到
//! `trap_entry_rust`。
//!
//! **须在** `task::init()` **之后**调用 [`init`]。

use abi::syscall_number::{ActiveSyscallNumberTable, SyscallNumberTable};
use abi::user_ret::UserRet;
use arch_api_v0::kernel_trap::register_kernel_trap_handler;
use arch_api_v0::trap::{
    Exception, Interrupt, TrapCause, TrapFrameRead, TrapFrameWrite, TrapSyscallRead,
    TrapSyscallWrite,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use platform::arch::paging;
use platform::arch::trap::ActiveTrapFrame as TrapContext;
use runtime::logging::*;
use syscall::dispatch_syscall_from_trap;

/// 监督态定时器中断后，用 **与 `kernel_main` 相同的 wall-clock 语义**
/// 重新武装固件定时器。
///
/// 先前使用 `read_time_tick() + TIMER_SLICE_TICKS` 的 **裸 `time` CSR
/// 增量**；在部分 QEMU/频率 组合下若与 SBI `set_timer`
/// 期望的刻度或固件内部换算不一致，可能导致 **deadline 落在「现在」之前**
/// 或极近，从而 **S 态定时器中断连发**。此时 GDB `SIGINT` 常在
/// `trampoline_start`（`__alltraps`） 处采样到 PC，看起来像「卡在
/// trampoline」，实为 **trap 风暴**而非 `sret` 损坏。
const TIMER_REARM_MS : u64 = 10;
static TIMER_TICK_COUNT : AtomicUsize = AtomicUsize::new(0);
/// 当前支持架构的 syscall/trap 指令宽度，用于将用户 PC 前进到下一条指令。
const SYSCALL_INSN_BYTES : usize = 4;

/// 记录用户任务 trap 杀进程上下文并终止当前任务。
fn kill_current_user_task(context : &str, trap_cause : TrapCause, cx : &TrapContext) -> ! {
    if let Some(snapshot) = task::current_task_snapshot() {
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
    task::exit_current(-1);
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
        #[cfg(any(feature = "impl-sv39", feature = "impl-loongarch64"))]
        paging::activate_address_space_token_and_flush(mm::kernel_mm::kernel_satp());
        platform::arch::trap::prepare_user_trap_frame_access();
    }
    let raw_cause = cx.raw_cause();
    let trap_cause = cx.trap_cause();
    match trap_cause {
        TrapCause::Exception(Exception::UserEnvCall) => {
            let syscall_nr = cx.syscall_nr()
                               .raw();
            let syscall_args = cx.syscall_args();
            let regs = syscall_args.as_regs();
            trace!("[syscall] nr={} user_pc={:#x} user_sp={:#x} \
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
            let syscall_ret = dispatch_syscall_from_trap(syscall_nr, syscall_args);
            if syscall_ret < 0 {
                warn!("[syscall] syscall failed ! nr={} ret={}",
                      syscall_nr, syscall_ret);
            } else {
                trace!("[syscall] nr={} ret={}",
                       syscall_nr,
                       syscall_ret);
            }
            // execve 已替换整个 trap 帧，跳过 sepc 推进与返回值写入
            if syscall_nr != <ActiveSyscallNumberTable as SyscallNumberTable>::EXEC.raw() {
                cx.add_user_pc(SYSCALL_INSN_BYTES);
                cx.set_syscall_ret(UserRet(syscall_ret));
            }
        }
        TrapCause::Exception(Exception::InstructionPageFault) |
        TrapCause::Exception(Exception::LoadPageFault) |
        TrapCause::Exception(Exception::StorePageFault) => {
            // 先前仅 `debug!` 并继续 `sret`：若 fault 发生在用户态，会回到同一 sepc 立即再
            // fault， 形成无限 trap 风暴；在 INFO
            // 日志级别下毫无输出，表现为「sret 后卡死」。
            if cx.returns_to_user() {
                warn!("[trap] user memory fault {:?} raw={:#x} ecode={:#x} sepc={:#x} \
                       stval={:#x} user_sp={:#x} — killing task",
                      trap_cause,
                      raw_cause,
                      (raw_cause >> 16) & 0x3F,
                      cx.user_pc(),
                      cx.fault_addr(),
                      cx.user_sp());
                kill_current_user_task("user memory fault", trap_cause, cx);
            }
            fatal_kernel_trap("kernel page fault",
                              trap_cause,
                              raw_cause,
                              cx);
        }
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            if let Err(err) = platform::timer::set_timer_after_ms(TIMER_REARM_MS) {
                panic!("failed to re-arm timer in trap: {:?}",
                       err);
            }
            let tick = TIMER_TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if tick % 8 == 0 {
                trace!("[trap] timer tick {}", tick);
            }
            task::schedule_tick();
        }
        _ => {
            if cx.returns_to_user() {
                // 用户态异常（非法指令、断点等）：杀死当前任务而非 panic 内核
                kill_current_user_task("user exception", trap_cause, cx);
            }
            fatal_kernel_trap("unexpected trap",
                              trap_cause,
                              raw_cause,
                              cx);
        }
    }

    if cx.returns_to_user() {
        let address_space_token = paging::active_address_space_token();
        // `raw_cause` 来自 TrapContext.scause 快照，即 **本次** 进入内核的原因（如
        // ecall=0x8）， 不是硬件 CSR 的“下一异常预告”；`sret`
        // 前也不会用该槽位预测下一次 trap。
        trace!("[trap] sret to user pc={:#x} sp={:#x} address_space_token={:#x} \
                frame_scause={:#x} (this trap's scause snapshot)",
               cx.user_pc(),
               cx.user_sp(),
               address_space_token,
               raw_cause,);
    }

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
    trace!("[trap] restore_current_trap_frame restored={}",
           restored);
    if cx.returns_to_user() && !restored {
        panic!("restore_current_trap_frame failed before sret to user (current task trap_frame \
                missing?)");
    }
}

/// 向 `arch_api_v0` 注册本模块的 C ABI trap 回调；须在 `task::init()`
/// 之后调用。
#[inline]
pub fn init() { register_kernel_trap_handler(wateros_kernel_trap_handler); }
