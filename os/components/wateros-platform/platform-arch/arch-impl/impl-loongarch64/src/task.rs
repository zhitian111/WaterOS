//! LoongArch64 **任务上下文**：与 `asm/switch.S` 中 `__switch` 保存的 `ra`/`sp`/`$r22`–
//! `$r31` 顺序一致；`s[0]` 承载 `bootstrap_ptr`，由 `__arch_task_entry` 传入跳板。

use api_v0::task::ArchTaskContext;

unsafe extern "C" {
    fn __wateros_task_runtime_entry(bootstrap_ptr: usize) -> !;
    fn __wateros_task_runtime_enter_current_user_task() -> !;
}

/// 与 `switch.S` 一致的 callee-saved 子集 + `ra`/`sp`（共 10 个 `$r22`–`$r31` 槽）。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LoongArch64ArchTaskContext {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 10],
}

impl ArchTaskContext for LoongArch64ArchTaskContext {
    #[inline]
    fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 10],
        }
    }

    #[inline]
    fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self {
        Self {
            ra: entry_stub,
            sp: kstack_top,
            s: [0; 10],
        }
    }

    #[inline]
    fn goto_task_entry(entry_stub: usize, kstack_top: usize, bootstrap_ptr: usize) -> Self {
        let mut cx = Self::goto_entry(entry_stub, kstack_top);
        cx.s[0] = bootstrap_ptr;
        cx
    }

    #[inline]
    fn return_address(&self) -> usize { self.ra }

    #[inline]
    fn stack_pointer(&self) -> usize { self.sp }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_task_entry_trampoline(bootstrap_ptr: usize) -> ! {
    unsafe { __wateros_task_runtime_entry(bootstrap_ptr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_user_task_entry_trampoline() -> ! {
    unsafe { __wateros_task_runtime_enter_current_user_task() }
}
