#![no_std]

// 设备驱动能力向上统一可见（不做按子系统裁剪导出）
pub mod api {
    pub use ::api_v0::*;
}
pub mod block {
    pub use ::block::*;
}
pub mod character {
    pub use ::character::*;
}
pub mod network {
    pub use ::network::*;
}

#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use impl_qemu_riscv64_opensbi as active_impl;
#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;

pub fn init_when_boot(dtb_pa: usize) {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::init_when_boot(dtb_pa);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = dtb_pa;
    }
}

pub fn init_after_boot() {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        if let Err(e) = impl_qemu_riscv64_opensbi::init_after_boot() {
            log::warn!("[driver] init_after_boot failed: {:?}", e);
        }
    }
}

pub fn test() {
    log::trace!("[driver] test begin");
    api_v0::test();
    block::test();
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::test();
    #[cfg(feature = "impl-dummy")]
    log::info!("[driver] dummy impl: skip qemu probe test");
    log::trace!("[driver] test end");
}
