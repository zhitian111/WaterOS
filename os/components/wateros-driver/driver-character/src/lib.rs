//! 字符设备子系统入口：[`CharacterDevice`] 注册表与 DTB 声明表。

#![no_std]

extern crate alloc;

use alloc::string::String;
use driver_api::{DeviceType, SupportedDeviceEntry};

pub use api_v0::{
    character_device_at, character_device_count, first_character_device,
    register_character_device, with_character_device, CharacterDevice, CharacterReadFinish,
    CharacterReadReservation, SerialError, SerialPort, SerialPortCharacterDevice, SerialResult,
    SharedCharacterDevice,
};

/// 字符设备 API v0。
pub mod api_v0 {
    pub use character_api_v0::*;
}

#[cfg(feature = "impl-rtc-stub")]
pub use impl_rtc_stub::{register_rtc_stub, RtcCharacterDevice, RtcTime};

#[cfg(feature = "impl-null-stub")]
pub use impl_null_stub::{register_null_stub, NullCharacterDevice};

/// 字符子系统在 DTB 中声明可尝试绑定的设备。
pub const CHARACTER_SUPPORTED_DEVICES: &[SupportedDeviceEntry] = &[
    SupportedDeviceEntry {
        subsystem: "character",
        name: "ns16550a-mmio",
        compatible: "ns16550a",
    },
    SupportedDeviceEntry {
        subsystem: "character",
        name: "ns8250-mmio",
        compatible: "ns8250",
    },
];

/// 返回本子系统声明支持的设备条目。
pub fn supported_devices() -> &'static [SupportedDeviceEntry] {
    CHARACTER_SUPPORTED_DEVICES
}

/// 字符子系统是否声明可处理该 DTB 设备。
pub fn character_subsystem_claims_device(compatibles: &[String], probed: DeviceType) -> bool {
    if probed != DeviceType::Character {
        return false;
    }
    supported_devices().iter().any(|s| {
        s.subsystem == "character"
            && compatibles
                .iter()
                .any(|c| c.as_str() == s.compatible)
    })
}

/// 是否识别为 NS16550 类 UART 节点（用于 DTB 探测）。
pub fn is_uart_compatible(compatibles: &[String]) -> bool {
    compatibles.iter().any(|c| {
        matches!(
            c.as_str(),
            "ns16550a" | "ns8250" | "snps,dw-apb-uart"
        )
    })
}

#[cfg(all(feature = "impl-rtc-stub", feature = "impl-null-stub"))]
pub fn register_builtin_character_devices() {
    register_rtc_stub();
    register_null_stub();
}

#[cfg(all(feature = "impl-rtc-stub", not(feature = "impl-null-stub")))]
pub fn register_builtin_character_devices() {
    register_rtc_stub();
}

#[cfg(all(not(feature = "impl-rtc-stub"), feature = "impl-null-stub"))]
pub fn register_builtin_character_devices() {
    register_null_stub();
}

#[cfg(not(any(feature = "impl-rtc-stub", feature = "impl-null-stub")))]
pub fn register_builtin_character_devices() {}
