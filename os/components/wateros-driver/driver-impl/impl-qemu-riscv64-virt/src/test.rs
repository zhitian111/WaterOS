//! 只读自检：不重复 probe / 注册。

use core::sync::atomic::Ordering;

use api_v0::{DriverError, DriverResult};
use block::{BlockDevice, Lba, VirtioBlkDevice, BLOCK_SIZE};

use crate::{devfs, register::VIRTIO_BLK_MMIO, INIT_AFTER_BOOT_DONE};

/// 对已注册的首个 virtio-blk 执行块 0 读取自检；无设备时 [`DriverError::NotFound`]。
pub fn virtio_blk_probe_test() -> DriverResult<()> {
    // 先复制 MMIO 描述再释放注册表锁，设备初始化可能分配内存且不能在锁内进行。
    let blk = VIRTIO_BLK_MMIO.lock();
    let Some(mmio) = blk.first().copied() else {
        return Err(DriverError::NotFound);
    };
    drop(blk);
    let mut dev = VirtioBlkDevice::from_mmio(mmio)?;
    let mut buf = [0u8; BLOCK_SIZE];
    dev.read_blocks(Lba(0), &mut buf)?;
    log::info!("[driver] virtio-blk read block0 ok, first16={:02x?}", &buf[..16]);
    Ok(())
}

/// 驱动自检：只读检查已注册设备与 devfs；不重复 probe / 注册。
pub fn test() {
    log::trace!("[driver-impl-qemu] test begin");
    if !INIT_AFTER_BOOT_DONE.load(Ordering::Acquire) {
        log::warn!(
            "[driver-impl-qemu] test skipped: init_after_boot not completed"
        );
        return;
    }
    devfs::dump_device_and_devfs_info();
    let _ = virtio_blk_probe_test();
    log::trace!("[driver-impl-qemu] test end");
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver/impl-qemu-riscv64] self_test begin");
    test();
    log::info!("[driver/impl-qemu-riscv64] self_test complete");
}
