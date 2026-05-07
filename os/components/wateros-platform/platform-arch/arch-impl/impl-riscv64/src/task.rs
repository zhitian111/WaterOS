//! RISC-V 上 **任务切换 callee-saved** 与内核/用户入口跳板所需的 `ArchTaskContext` 具体布局。
//!
//! 与 `asm/switch.S` 中 `__switch` 所保存的 `ra`/`sp`/s0–s11 顺序一致；修改字段或顺序时必须同步汇编。

use api_v0::task::ArchTaskContext;

unsafe extern "C" {
    fn __wateros_task_runtime_entry(bootstrap_ptr : usize) -> !;
    fn __wateros_task_runtime_enter_current_user_task() -> !;
}

/// 与 `switch.S` / 任务运行库约定的 **保存现场**：`ra`、`sp` 与 12 个 callee-saved。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Riscv64ArchTaskContext {
    /// 返回地址（切换目标后 `ret` 所至）。
    pub ra : usize,
    /// 栈指针。
    pub sp : usize,
    /// `s0`–`s11` 共 12 个寄存器槽位。
    pub s : [usize; 12],
}


impl ArchTaskContext for Riscv64ArchTaskContext {
    #[inline]
    fn zero_init() -> Self {
        Self { ra : 0,
               sp : 0,
               s : [0; 12] }
    }

    #[inline]
    fn goto_entry(entry_stub : usize, kstack_top : usize) -> Self {
        Self { ra : entry_stub,
               sp : kstack_top,
               s : [0; 12] }
    }

    #[inline]
    fn goto_task_entry(entry_stub : usize, kstack_top : usize, bootstrap_ptr : usize) -> Self {
        let mut cx = Self::goto_entry(entry_stub, kstack_top);
        cx.s[0] = bootstrap_ptr;
        cx
    }
}

/// 内核任务首次运行入口：将 `bootstrap_ptr` 交给运行库 `__wateros_task_runtime_entry`。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_task_entry_trampoline(bootstrap_ptr : usize) -> ! {
    unsafe { __wateros_task_runtime_entry(bootstrap_ptr) }
}

/// 用户任务经 `sret` 进入后的 Rust 侧入口（由运行库接管页表与用户镜像）。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_user_task_entry_trampoline() -> ! {
    unsafe { __wateros_task_runtime_enter_current_user_task() }
}
