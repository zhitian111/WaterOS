//! `/dev/null` 虚拟字符设备：读 EOF，写丢弃。

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use character_api_v0::{
    register_character_device, CharacterDevice, CharacterDeviceKind, SharedCharacterDevice,
};
use driver_api::DriverResult;
use spin::Mutex;

/// POSIX `/dev/null` 语义。
#[derive(Debug, Clone, Copy, Default)]
pub struct NullCharacterDevice;

impl CharacterDevice for NullCharacterDevice {
    fn read(&mut self, _buf: &mut [u8]) -> DriverResult<usize> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> DriverResult<usize> {
        Ok(buf.len())
    }

    fn device_kind(&self) -> CharacterDeviceKind {
        CharacterDeviceKind::Null
    }
}

/// 注册全局 null stub 并返回设备索引。
pub fn register_null_stub() -> usize {
    let shared: SharedCharacterDevice =
        Arc::new(Mutex::new(Box::new(NullCharacterDevice)));
    register_character_device(shared)
}
