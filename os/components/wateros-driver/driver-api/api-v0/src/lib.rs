#![no_std]
extern crate alloc;

use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Block,
    Character,
    Network,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmioRegion {
    pub base: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqLine {
    pub irq: u32,
    pub parent: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub node_name: String,
    pub compatible: String,
    pub device_type: DeviceType,
    pub mmio: Option<MmioRegion>,
    pub irq: Option<IrqLine>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    InvalidDtb,
    InvalidParam,
    NotFound,
    Unsupported,
    IoError,
}

pub type DriverResult<T> = core::result::Result<T, DriverError>;

pub fn test() {
    log::trace!("[driver-api] test begin");
    let info = DeviceInfo {
        node_name: String::from("virtio_blk@10001000"),
        compatible: String::from("virtio,mmio"),
        device_type: DeviceType::Block,
        mmio: Some(MmioRegion {
            base: 0x1000_1000,
            size: 0x1000,
        }),
        irq: Some(IrqLine {
            irq: 1,
            parent: Some(0),
        }),
    };
    assert_eq!(info.device_type, DeviceType::Block);
    assert!(info.mmio.is_some());
    log::trace!("[driver-api] test end");
}
