use api_v0::task::ArchTaskContext;

unsafe extern "C" {
    fn __wateros_idle_task_entry() -> !;
    fn __wateros_task_entry(task_start_ptr: usize) -> !;
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Riscv64ArchTaskContext {
    pub ra: usize,
    pub sp: usize,
    pub s: [usize; 12],
}

impl Riscv64ArchTaskContext {
    #[inline]
    pub const fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }

    #[inline]
    pub const fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self {
        Self {
            ra: entry_stub,
            sp: kstack_top,
            s: [0; 12],
        }
    }

    #[inline]
    pub const fn goto_task_entry(
        entry_stub: usize,
        kstack_top: usize,
        task_start_ptr: usize,
    ) -> Self {
        let mut cx = Self::goto_entry(entry_stub, kstack_top);
        cx.s[0] = task_start_ptr;
        cx
    }
}

impl ArchTaskContext for Riscv64ArchTaskContext {
    #[inline]
    fn zero_init() -> Self { Self::zero_init() }

    #[inline]
    fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self {
        Self::goto_entry(entry_stub, kstack_top)
    }

    #[inline]
    fn goto_task_entry(entry_stub: usize, kstack_top: usize, task_start_ptr: usize) -> Self {
        Self::goto_task_entry(entry_stub, kstack_top, task_start_ptr)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_task_entry_trampoline(task_start_ptr: usize) -> ! {
    unsafe { __wateros_task_entry(task_start_ptr) }
}

#[unsafe(no_mangle)]
pub extern "C" fn __wateros_arch_idle_task_entry_trampoline() -> ! {
    unsafe { __wateros_idle_task_entry() }
}
