//! 网络设备子系统：DTB 绑定声明、网络 API 再导出，以及可选 NIC 实现。
//!
//! 扫描阶段仅依赖 [`NETWORK_SUPPORTED_DEVICES`] 与 [`supported_devices`]；具体网卡驱动在启用对应 feature 时提供。

#![no_std]
extern crate alloc;

use alloc::string::String;

pub use api_v0::*;
pub use driver_api::{DeviceType, SupportedDeviceEntry};

#[cfg(feature = "impl-dummy")]
#[doc(inline)]
pub use impl_dummy::DummyNetworkDevice;
#[cfg(feature = "impl-virtio-mmio")]
#[doc(inline)]
pub use impl_virtio_mmio::VirtioNetDevice;
#[cfg(feature = "impl-virtio-pci")]
#[doc(inline)]
pub use impl_virtio_pci::{VirtioNetPciBarAllocator, VirtioNetPciProbeInfo, VirtioPciNetDevice};

/// 网络子系统在 DTB 中声明可尝试绑定的设备（与 feature 无关；用于扫描阶段匹配）。
pub const NETWORK_SUPPORTED_DEVICES : &[SupportedDeviceEntry] =
    &[SupportedDeviceEntry { subsystem : "network",
                             name : "virtio-net-mmio",
                             compatible : "virtio,mmio" },
      SupportedDeviceEntry { subsystem : "network",
                             name : "virtio-net-pci-transitional",
                             compatible : "pci1af4,1000" },
      SupportedDeviceEntry { subsystem : "network",
                             name : "virtio-net-pci-modern",
                             compatible : "pci1af4,1041" }];

/// 返回本子系统声明支持的设备条目（非排他；可与其它子系统条目并存）。
pub fn supported_devices() -> &'static [SupportedDeviceEntry] { NETWORK_SUPPORTED_DEVICES }

/// 网络子系统是否声明可处理该 DTB 设备（仅基于 `compatible` 列表与探测到的 [`DeviceType`]，不含具体初始化成败）。
pub fn network_subsystem_claims_device(compatibles : &[String], probed : DeviceType) -> bool {
    if probed != DeviceType::Network {
        return false;
    }
    supported_devices().iter()
                       .any(|s| {
                           s.subsystem == "network" &&
                           compatibles.iter()
                                      .any(|c| c.as_str() == s.compatible)
                       })
}

/// 调用网络 API 自带自检（不访问真实硬件）。
pub fn test() {
    log::trace!("[driver-network] test begin");
    api_v0::test();
    log::trace!("[driver-network] test end");
}
