//! WaterOS 设备驱动聚合层：统一导出子系统 API、可选平台实现与引导期入口。
//!
//! `api`、`block`、`character`、`network`
//! 模块不做按子系统裁剪，便于上层一次性依赖；具体硬件绑定由 feature 选中的
//! [`active_impl`] 完成。

#![no_std]

// `supported_device_entries` 等路径需要 `Vec`；子 crate 各自声明 `extern crate
// alloc`。
extern crate alloc;

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

// 三选一：QEMU/OpenSBI RISC-V, QEMU/LoongArch64 virt, 或 dummy
// 占位（可同时不选，则无 `active_impl` 符号）。
#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;
#[cfg(feature = "impl-qemu-loongarch64-virt")]
pub use impl_qemu_loongarch64_virt as active_impl;
#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use impl_qemu_riscv64_opensbi as active_impl;
#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use impl_qemu_riscv64_opensbi::uart;

use alloc::vec::Vec;
use api_v0::SupportedDeviceEntry;

/// 合并各子系统 [`supported_devices()`]
/// 的静态条目，便于诊断「内核声明了哪些可绑定设备」。
pub fn supported_device_entries() -> Vec<&'static SupportedDeviceEntry> {
    block::supported_devices().iter()
                              .chain(character::supported_devices())
                              .chain(network::supported_devices())
                              .collect()
}

/// 引导早期调用：保存 DTB 物理地址等平台状态；具体解析在 [`init_after_boot`]
/// 或各实现内完成。
pub fn init_when_boot(dtb_pa : usize) {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::init_when_boot(dtb_pa);
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    impl_qemu_loongarch64_virt::init_when_boot(dtb_pa);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = dtb_pa;
    }
}

/// 物理 RAM 上界（不包含），用于恒等映射与帧分配器；QEMU 实现从 DTB
/// 解析，其它配置返回回退常量。
#[inline]
pub fn physical_ram_end_exclusive() -> usize {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        impl_qemu_riscv64_opensbi::physical_ram_end_exclusive()
    }
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        impl_qemu_loongarch64_virt::physical_ram_end_exclusive()
    }
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt")))]
    {
        wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_END
    }
}

/// 内核完成必要子系统初始化后调用：扫描/注册设备等；失败时记录日志，
/// 不向上返回错误（当前契约）。
pub fn init_after_boot() {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        if let Err(e) = impl_qemu_riscv64_opensbi::init_after_boot() {
            log::warn!("[driver] init_after_boot failed: {:?}",
                       e);
        }
    }
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        if let Err(e) = impl_qemu_loongarch64_virt::init_after_boot() {
            log::warn!("[driver] init_after_boot failed: {:?}",
                       e);
        }
    }
}

/// 自检入口：依次调用 API 与各子系统测试钩子；QEMU 实现会跑探测路径，dummy
/// 实现跳过硬件。
pub fn test() {
    log::trace!("[driver] test begin");
    api_v0::test();
    block::test();
    network::test();
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::test();
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    impl_qemu_loongarch64_virt::test();
    #[cfg(feature = "impl-dummy")]
    log::info!("[driver] dummy impl: skip qemu probe test");
    log::trace!("[driver] test end");
}
