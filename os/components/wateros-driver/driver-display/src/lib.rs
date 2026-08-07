//! WaterOS 显示驱动子系统聚合入口。
//!
//! API 始终可用，具体 VirtIO transport 由平台 feature 选择。

#![no_std]

pub use api_v0::*;

/// 显示 API 的显式命名空间。
pub mod api_v0 {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-virtio-mmio")]
pub use impl_virtio_mmio::VirtioGpuMmioDevice;
#[cfg(feature = "impl-virtio-pci")]
pub use impl_virtio_pci::{VirtioGpuPciBarAllocator, VirtioGpuPciDevice, VirtioGpuPciProbeInfo};

use driver_api::{DeviceType, SupportedDeviceEntry};

static SUPPORTED : [SupportedDeviceEntry; 3] = [
    SupportedDeviceEntry { subsystem : "display",
                           name : "virtio-gpu-mmio",
                           compatible : "virtio,mmio" },
    SupportedDeviceEntry { subsystem : "display",
                           name : "virtio-gpu-pci-transitional",
                           compatible : "pci1af4,1010" },
    SupportedDeviceEntry { subsystem : "display",
                           name : "virtio-gpu-pci-modern",
                           compatible : "pci1af4,1050" },
];

/// 返回显示子系统声明的可绑定设备。
pub fn supported_devices() -> &'static [SupportedDeviceEntry] { &SUPPORTED }

/// 判断平台枚举结果是否应交给显示子系统。
pub fn display_subsystem_claims_device(compatibles : &[alloc::string::String],
                                       device_type : DeviceType)
                                       -> bool {
    device_type == DeviceType::Display &&
    compatibles.iter()
               .any(|compatible| compatible == "virtio,mmio")
}

extern crate alloc;
