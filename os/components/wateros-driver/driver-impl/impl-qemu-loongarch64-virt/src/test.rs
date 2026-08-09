//! 只读自检：不重复 probe / 注册。

use core::sync::atomic::Ordering;

use api_v0::{DriverError, DriverResult};
use block::{first_block_device, BlockDevice, Lba, BLOCK_SIZE};

use crate::INIT_AFTER_BOOT_DONE;

/// 对已注册的首个 virtio-blk 执行块 0 读取自检。
pub fn virtio_blk_probe_test() -> DriverResult<()> {
    let Some(dev) = first_block_device() else {
        return Err(DriverError::NotFound);
    };
    let mut dev = dev.lock();
    let mut buf = [0u8; BLOCK_SIZE];
    dev.read_blocks(Lba(0), &mut buf)?;
    log::info!(
        "[driver-la] virtio-blk read block0 ok, first16={:02x?}",
        &buf[..16]
    );
    Ok(())
}

/// 驱动自检：只读块 0 自检；不重复 probe / 注册。
pub fn test() {
    log::trace!("[driver-la] test begin");
    if !INIT_AFTER_BOOT_DONE.load(Ordering::Acquire) {
        log::warn!("[driver-la] test skipped: init_after_boot not completed");
        return;
    }
    let _ = virtio_blk_probe_test();
    log::trace!("[driver-la] test end");
}
