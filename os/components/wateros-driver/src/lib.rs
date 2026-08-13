//! WaterOS 设备驱动聚合层：统一导出子系统 API、可选平台实现与引导期入口。
//!
//! `api`、`block`、`character`、`display`、`input`、`network`
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
pub mod display {
    pub use ::display::*;
}
pub mod input {
    pub use ::input::*;
}
pub mod network {
    pub use ::network::*;
}

// 二选一：QEMU/OpenSBI RISC-V 或 QEMU/LoongArch64 virt；
// 行为契约统一由 [`MachineDriver`] 表达，经 [`machine`] 选择当前 profile。
#[cfg(feature = "impl-qemu-loongarch64-virt")]
pub use impl_qemu_loongarch64_virt::uart;
#[cfg(feature = "impl-qemu-riscv64-virt")]
pub use impl_qemu_riscv64_virt::uart;

use alloc::vec::Vec;
use api_v0::{MachineDriver, SupportedDeviceEntry};

/// 当前 feature 选中的机器驱动契约实现（QEMU RV/LA）。
pub fn machine() -> &'static dyn MachineDriver {
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        impl_qemu_loongarch64_virt::machine()
    }
    #[cfg(feature = "impl-qemu-riscv64-virt")]
    {
        impl_qemu_riscv64_virt::machine()
    }
    #[cfg(not(any(feature = "impl-qemu-loongarch64-virt",
                  feature = "impl-qemu-riscv64-virt")))]
    {
        core::unreachable!("no machine driver feature selected")
    }
}

/// 合并各子系统 [`supported_devices()`]
/// 的静态条目，便于诊断「内核声明了哪些可绑定设备」。
pub fn supported_device_entries() -> Vec<&'static SupportedDeviceEntry> {
    block::supported_devices().iter()
                              .chain(character::supported_devices())
                              .chain(display::supported_devices())
                              .chain(input::supported_devices())
                              .chain(network::supported_devices())
                              .collect()
}

/// 内核完成必要子系统初始化后调用：扫描/注册设备等；失败时记录日志，
/// 不向上返回错误（当前契约）。
pub fn init_after_boot() {
    if let Err(e) = machine().init_after_boot() {
        log::warn!("[driver] init_after_boot failed: {:?}",
                   e);
    }
}

/// 启动前驱动阶段入口；仅准备聚合层，不访问尚未探测的设备。
pub fn init_when_boot() {
    log::info!("[driver] init_when_boot: driver facade ready");
}

/// 自检入口：依次调用 API 与各子系统测试钩子。
pub fn test() {
    log::trace!("[driver] test begin");
    api_v0::test();
    block::test();
    character::api_v0::test();
    assert_eq!(display::supported_devices().len(), 3);
    network::test();
    machine().test();
    log::trace!("[driver] test end");
}

/// 驱动组件统一内核态自检入口；仅探测和验证已注册的内核设备能力。
#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver] self_test begin");
    api_v0::test();
    block::test();
    character::api_v0::test();
    assert_eq!(display::supported_devices().len(), 3);
    network::test();
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    impl_qemu_loongarch64_virt::self_test();
    #[cfg(feature = "impl-qemu-riscv64-virt")]
    impl_qemu_riscv64_virt::self_test();
    log::info!("[driver] self_test complete");
}
