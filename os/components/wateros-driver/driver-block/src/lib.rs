//! 块设备子系统：DTB/PCI 绑定声明、块 API 再导出，以及可选 VirtIO 实现。
//!
//! 扫描阶段仅依赖 [`BLOCK_SUPPORTED_DEVICES`] 与 [`supported_devices`]；具体块驱动在启用对应 feature 时提供。

#![no_std]
extern crate alloc;

use alloc::string::String;

pub use api_v0::*;
pub use driver_api::{DeviceType, SupportedDeviceEntry};

#[cfg(feature = "impl-virtio-mmio")]
#[doc(inline)]
pub use impl_virtio_mmio::VirtioBlkDevice;

#[cfg(feature = "impl-virtio-pci")]
#[doc(inline)]
pub use impl_virtio_pci::{VirtioPciBarAllocator, VirtioPciBlkDevice, VirtioPciProbeInfo};

#[cfg(feature = "impl-block-cache")]
#[doc(inline)]
pub use impl_block_cache::{BlockCacheConfig, BlockCacheManager, CachingBlockDevice};

/// 块子系统在 DTB 中声明可尝试绑定的设备（与 feature 无关；用于扫描阶段匹配）。
pub const BLOCK_SUPPORTED_DEVICES: &[SupportedDeviceEntry] = &[
    SupportedDeviceEntry {
        subsystem: "block",
        name: "virtio-blk-mmio",
        compatible: "virtio,mmio",
    },
    SupportedDeviceEntry {
        subsystem: "block",
        name: "virtio-blk-pci-transitional",
        compatible: "pci1af4,1001",
    },
    SupportedDeviceEntry {
        subsystem: "block",
        name: "virtio-blk-pci-modern",
        compatible: "pci1af4,1042",
    },
];

/// 返回本子系统声明支持的设备条目（非排他；可与其它子系统条目并存）。
pub fn supported_devices() -> &'static [SupportedDeviceEntry] {
    BLOCK_SUPPORTED_DEVICES
}

/// 块子系统是否声明可处理该 DTB 设备（仅基于 `compatible` 列表与探测到的 [`DeviceType`]，不含具体初始化成败）。
pub fn block_subsystem_claims_device(compatibles: &[String], probed: DeviceType) -> bool {
    if probed != DeviceType::Block {
        return false;
    }
    // 与 `BLOCK_SUPPORTED_DEVICES` 中 `compatible` 精确匹配即可；不要求节点名或 reg 形态。
    supported_devices()
        .iter()
        .any(|s| {
            s.subsystem == "block"
                && compatibles
                    .iter()
                    .any(|c| c.as_str() == s.compatible)
        })
}

/// 调用块 API 自带自检（不访问真实硬件）。
pub fn test() {
    log::trace!("[driver-block] test begin");
    api_v0::test();
    log::trace!("[driver-block] test end");
}
