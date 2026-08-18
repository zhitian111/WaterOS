//! WaterOS 输入设备聚合层。

#![no_std]
extern crate alloc;

pub use api_v0::*;

#[cfg(feature = "impl-virtio-mmio")]
pub use impl_virtio_mmio::VirtioInputMmioDevice;
#[cfg(feature = "impl-virtio-pci")]
pub use impl_virtio_pci::{VirtioInputPciBarAllocator, VirtioInputPciDevice, VirtioInputPciProbeInfo};

use driver_api::{DeviceType, SupportedDeviceEntry};

static SUPPORTED : [SupportedDeviceEntry; 3] = [
    SupportedDeviceEntry { subsystem : "input",
                           name : "virtio-input-mmio",
                           compatible : "virtio,mmio" },
    SupportedDeviceEntry { subsystem : "input",
                           name : "virtio-input-pci-transitional",
                           compatible : "pci1af4,1012" },
    SupportedDeviceEntry { subsystem : "input",
                           name : "virtio-input-pci-modern",
                           compatible : "pci1af4,1052" },
];

pub fn supported_devices() -> &'static [SupportedDeviceEntry] { &SUPPORTED }

pub fn input_subsystem_claims_device(compatibles : &[alloc::string::String],
                                     device_type : DeviceType)
                                     -> bool {
    // 仅凭设备类型和完整兼容字符串声明归属，不把探测成功等同于初始化成功。
    device_type == DeviceType::Input &&
    compatibles.iter().any(|compatible| compatible == "virtio,mmio")
}
