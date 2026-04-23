use api_v0::KernelTaskEntry;

/// Opaque bootstrap payload handed from arch task-entry trampolines to the
/// task runtime. This stays in the implementation layer so public task APIs do
/// not need to expose startup protocol details.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TaskBootstrap {
    pub entry: KernelTaskEntry,
    pub arg: usize,
}

impl TaskBootstrap {
    #[inline]
    pub const fn new(entry: KernelTaskEntry, arg: usize) -> Self { Self { entry, arg } }

    #[inline]
    pub fn run(&self) -> ! { (self.entry)(self.arg) }
}
