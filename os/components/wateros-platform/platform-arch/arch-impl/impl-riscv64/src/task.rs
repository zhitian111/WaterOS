use api_v0::task::ArchTaskContext;

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
}

impl ArchTaskContext for Riscv64ArchTaskContext {
    #[inline]
    fn zero_init() -> Self { Self::zero_init() }

    #[inline]
    fn goto_entry(entry_stub: usize, kstack_top: usize) -> Self {
        Self::goto_entry(entry_stub, kstack_top)
    }
}
