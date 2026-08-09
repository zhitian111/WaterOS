use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct Loongson2K1000LABootArgs {
    arg0 : usize,
    arg1 : usize,
    arg2 : usize,
}

impl Loongson2K1000LABootArgs {
    pub const fn new(arg0 : usize, arg1 : usize, arg2 : usize) -> Self { Self { arg0, arg1, arg2 } }
}

impl PlatformBootArgs for Loongson2K1000LABootArgs {
    fn arg0(&self) -> Option<usize> { Some(self.arg0) }
    fn arg1(&self) -> Option<usize> { Some(self.arg1) }
    fn arg2(&self) -> Option<usize> { Some(self.arg2) }
}

/// No fixed DTB exists in the documented legacy boot ABI.
pub const fn device_tree_phys_addr() -> usize { 0 }

pub use Loongson2K1000LABootArgs as BootArgs;
