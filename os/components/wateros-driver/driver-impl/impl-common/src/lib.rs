//! 机器驱动共用的枚举/解析原语。
//!
//! 只放与具体 transport 无关、多个 driver-impl profile（QEMU RV/LA 及后续
//! 真机板级）会复用的代码；transport 专用探测仍留在各 driver-impl 的
//! `enumerate` 模块。

#![no_std]
extern crate alloc;

pub mod dtb;
pub mod virtio_hal;
