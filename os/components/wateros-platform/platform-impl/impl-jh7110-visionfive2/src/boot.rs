use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct VisionFive2BootArgs {
    hart_id : usize,
    dtb_pa : usize,
}

impl VisionFive2BootArgs {
    pub const fn new(hart_id : usize, dtb_pa : usize) -> Self { Self { hart_id, dtb_pa } }
}

impl PlatformBootArgs for VisionFive2BootArgs {
    fn arg0(&self) -> Option<usize> { Some(self.hart_id) }
    fn arg1(&self) -> Option<usize> { Some(self.dtb_pa) }
}

pub use VisionFive2BootArgs as BootArgs;
