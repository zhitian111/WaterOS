//! QEMU LoongArch64 `virt` 平台驱动：PCIe ECAM 枚举、virtio 注册与 devfs 同步。
//!
//! 职责划分：`boot` 保存引导 DTB 并初始化早期 UART；`enumerate` 做 PCIe ECAM
//! 扫描；`register` 实例化并注册各子系统设备；`devfs` 同步设备视图；`test`
//! 提供只读自检；`machine` 以 [`MachineDriver`] 契约对外暴露本 profile。

#![no_std]
extern crate alloc;

mod boot;
mod devfs;
mod enumerate;
mod machine;
mod register;
mod test;
pub mod uart;

use core::sync::atomic::{AtomicBool, Ordering};

use api_v0::DriverResult;
use block::block_device_count;
use character::character_device_count;
use network::network_device_count;
#[cfg(feature = "display")]
use display::display_device_count;
#[cfg(feature = "input")]
use input::input_device_count;

/// 防止重复 bring-up；成功后供 `test` 等路径读取。
pub(crate) static INIT_AFTER_BOOT_DONE: AtomicBool = AtomicBool::new(false);

pub use machine::machine;

/// 扫描 PCIe ECAM 总线、注册 virtio 设备并同步 devfs。
pub fn init_after_boot() -> DriverResult<()> {
    if INIT_AFTER_BOOT_DONE.swap(true, Ordering::AcqRel) {
        log::warn!(
            "[lock-audit][platform-probe] duplicate init_after_boot ignored \
             (platform=loongarch64-virt)"
        );
        return Ok(());
    }

    let result = init_after_boot_inner();
    if result.is_err() {
        INIT_AFTER_BOOT_DONE.store(false, Ordering::Release);
    }
    result
}

fn init_after_boot_inner() -> DriverResult<()> {
    for e in block::supported_devices() {
        log::info!(
            "[driver-la] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }
    for e in network::supported_devices() {
        log::info!(
            "[driver-la] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }
    #[cfg(feature = "display")]
    for e in display::supported_devices() {
        log::info!(
            "[driver-la] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem, e.name, e.compatible
        );
    }
    #[cfg(feature = "input")]
    for e in input::supported_devices() {
        log::info!(
            "[driver-la] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem, e.name, e.compatible
        );
    }

    register::register_devices()?;

    let registered = block_device_count();
    let registered_net = network_device_count();
    let registered_chr = character_device_count();
    #[cfg(feature = "display")]
    let registered_display = display_device_count();
    #[cfg(feature = "input")]
    let registered_input = input_device_count();
    log::info!(
        "[driver-la] devices registered: block={} network={} character={}",
        registered,
        registered_net,
        registered_chr
    );
    #[cfg(feature = "display")]
    log::info!("[driver-la] display devices registered: count={}", registered_display);
    #[cfg(feature = "input")]
    log::info!("[driver-la] input devices registered: count={}", registered_input);
    if registered == 0 {
        log::warn!(
            "[driver-la] no block device registered; root fs may use NotMounted \
                        unless a virtio-blk is present. QEMU example: `-device \
                        virtio-blk-pci,drive=x0 -drive file=...,if=none,format=raw,id=x0`."
        );
    } else if let Err(err) = test::virtio_blk_probe_test() {
        log::warn!(
            "[driver-la] virtio-blk block0 read self-test failed: {:?}",
            err
        );
    }
    if registered_net == 0 {
        log::warn!(
            "[driver-la] no network device registered; NIC may not be present. \
                        QEMU example: `-netdev user,id=net0 -device virtio-net-pci,netdev=net0`."
        );
    }

    devfs::sync();
    uart::init_default_virt_uart();
    log::info!("[driver-la] QEMU LoongArch64 UART16550 ready (serial I/O)");

    Ok(())
}
