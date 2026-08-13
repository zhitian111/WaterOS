//! QEMU `virt` 机器、RISC-V64、OpenSBI 环境下的机器驱动。
//!
//! 职责划分：`enumerate` 扫描 DTB 设备表；`register`
//! 实例化并注册各子系统设备；`devfs` 同步设备视图；`test` 提供只读自检；
//! `machine` 以 [`MachineDriver`] 契约对外暴露本 profile；`uart` 负责平台侧
//! UART 接线。

#![no_std]
extern crate alloc;

pub mod devfs;
pub mod enumerate;
pub mod machine;
pub mod register;
pub mod test;
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

#[cfg(feature = "self_test")]
pub fn self_test() {
    test::self_test();
}

/// DTB 扫描、virtio 注册与 devfs 同步的完整 bring-up 路径；成功返回后设备表可能仍为空。
pub fn init_after_boot() -> DriverResult<()> {
    if INIT_AFTER_BOOT_DONE.swap(true, Ordering::AcqRel) {
        log::warn!(
            "[lock-audit][platform-probe] duplicate init_after_boot ignored \
             (platform=riscv64-opensbi)"
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
            "[driver] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }
    for e in network::supported_devices() {
        log::info!(
            "[driver] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }
    for e in character::supported_devices() {
        log::info!(
            "[driver] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }
    #[cfg(feature = "display")]
    for e in display::supported_devices() {
        log::info!(
            "[driver] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }
    #[cfg(feature = "input")]
    for e in input::supported_devices() {
        log::info!("[driver] supported-device catalog: subsystem={} name={} compatible={}",
                   e.subsystem, e.name, e.compatible);
    }

    let count = enumerate::scan_device_info()?;
    log::trace!("[driver] dtb scan done, devices={}", count);
    register::probe_character_devices();
    let unsupported = register::probe_virtio_devices();
    let registered_blk = block_device_count();
    let registered_net = network_device_count();
    let registered_chr = character_device_count();
    #[cfg(feature = "display")]
    let registered_display = display_device_count();
    #[cfg(feature = "input")]
    let registered_input = input_device_count();
    log::info!(
        "[driver] devices registered: block={} network={} character={}",
        registered_blk,
        registered_net,
        registered_chr
    );
    #[cfg(feature = "display")]
    log::info!("[driver] display devices registered: count={}", registered_display);
    #[cfg(feature = "input")]
    log::info!("[driver] input devices registered: count={}", registered_input);
    if registered_blk == 0 {
        log::warn!(
            "[driver] no block device registered; root fs may use NotMounted unless a virtio-blk is present. \
             QEMU virt example: `-drive file=...,if=none,id=d0 -device virtio-blk-device,drive=d0`."
        );
    }
    if registered_net == 0 {
        log::warn!(
            "[driver] no network device registered; NIC may not be present. \
             QEMU virt example: `-netdev user,id=n0 -device virtio-net-device,netdev=n0`."
        );
    }
    devfs::sync(unsupported);
    log::info!("[driver] QEMU virt UART0 MMIO ready (serial I/O)");
    Ok(())
}
