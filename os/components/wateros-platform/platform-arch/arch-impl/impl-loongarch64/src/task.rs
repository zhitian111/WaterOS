use api_v0::task::ArchTaskContext;

unsafe extern "C" {
    fn __wateros_task_runtime_entry(bootstrap_ptr: usize) -> !;
    fn __wateros_task_runtime_enter_current_user_task() -> !;
}

/// LoongArch64 LP64 任务切换上下文。
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
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_task_entry_trampoline(bootstrap_ptr: usize) -> ! {
    unsafe { __wateros_task_runtime_entry(bootstrap_ptr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_user_task_entry_trampoline() -> ! {
    unsafe { __wateros_task_runtime_enter_current_user_task() }
}
