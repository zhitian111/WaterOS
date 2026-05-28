#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{DriverError, DriverResult};
use block::{
    block_device_count, first_block_device, register_block_device, BlockDevice, Lba,
    VirtioPciProbeInfo, BLOCK_SIZE,
};
use fs::devfs::active_impl as devfs_impl;
use spin::Mutex;

mod pci;

/// DTB 物理基址；为 0 时退化至硬编码 PCI 配置空间基址。
static DTB_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
/// 成功注册为 virtio-blk 的 PCI 设备列表。
static VIRTIO_BLK_PCI: Mutex<Vec<VirtioPciProbeInfo>> = Mutex::new(Vec::new());

pub fn init_when_boot(dtb_pa: usize) {
    DTB_BASE_ADDR.store(dtb_pa, Ordering::Release);
}

/// 物理 RAM 上界（不包含）：loongarch64 virt 当前硬编码为 2 GiB（与 QEMU `-m
/// 2G` 及 `kernel_main` 一致）。
pub fn physical_ram_end_exclusive() -> usize {
    let _fallback = wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_END;
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb != 0 {
        if let Ok(fdt) = read_fdt() {
            let mut best_end = 0usize;
            for node in fdt.all_nodes() {
                if !node
                    .name
                    .starts_with("memory")
                {
                    continue;
                }
                let Some(mut regions) = node.reg() else {
                    continue;
                };
                while let Some(region) = regions.next() {
                    let base = region.starting_address as usize;
                    let Some(size) = region.size else {
                        continue;
                    };
                    let end = base.saturating_add(size);
                    if end > base && base >= 0x8000_0000 && end > best_end {
                        best_end = end;
                    }
                }
            }
            if best_end > 0x8000_0000 {
                return best_end;
            }
        }
    }
    // 回退：LoongArch RAM 基址 0x9000_0000 + 2 GiB = 0x1_1000_0000
    0x9000_0000usize.saturating_add(0x8000_0000)
}

fn read_fdt() -> DriverResult<fdt::Fdt<'static>> {
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb == 0 {
        return Err(DriverError::NotFound);
    }
    let fdt =
        unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
    Ok(fdt)
}

/// 扫描 PCIe ECAM 总线寻找 virtio-blk 设备并注册。
pub fn init_after_boot() -> DriverResult<()> {
    for e in block::supported_devices() {
        logging::info!(
            "[driver-la] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }

    let mut blk = VIRTIO_BLK_PCI.lock();
    blk.clear();

    // 尝试 PCIe ECAM 枚举 virtio-blk 设备。
    let config_base = pci::find_config_base(DTB_BASE_ADDR.load(Ordering::Acquire));
    logging::info!(
        "[driver-la] PCI config base = {:#x}",
        config_base
    );
    match pci::probe_virtio_blk_pci(config_base) {
        Ok(Some((dev, info))) => {
            let shared = {
                let dev: Box<dyn BlockDevice> = Box::new(dev);
                Arc::new(Mutex::new(dev))
            };
            let idx = register_block_device(shared);
            blk.push(info);
            logging::info!(
                "[driver-la] registered virtio-blk #{} via PCI {}:{}.{} vendor={:#06x} \
                            device={:#06x}",
                idx,
                info.bus,
                info.device,
                info.function,
                info.vendor_id,
                info.device_id
            );
        }
        Ok(None) => {
            logging::warn!("[driver-la][pci] no virtio-blk device found on PCI bus 0");
        }
        Err(err) => {
            logging::warn!(
                "[driver-la] failed to init virtio-blk via PCI: {:?}",
                err
            );
        }
    }

    let registered = block_device_count();
    logging::info!(
        "[driver-la] block devices registered={}",
        registered
    );
    if registered == 0 {
        logging::warn!(
            "[driver-la] no block device registered; root fs may use NotMounted \
                        unless a virtio-blk is present. QEMU example: `-device \
                        virtio-blk-pci,drive=x0 -drive file=...,if=none,format=raw,id=x0`."
        );
    } else if let Err(err) = virtio_blk_probe_test() {
        logging::warn!(
            "[driver-la] virtio-blk block0 read self-test failed: {:?}",
            err
        );
    }

    // devfs 同步：填充 /dev/sys/* 视图
    let node_count = devfs_impl::refresh();
    logging::info!(
        "[driver-la] devfs refreshed, nodes={}",
        node_count
    );

    Ok(())
}

/// 对已注册的首个 virtio-blk 执行块 0 读取自检。
pub fn virtio_blk_probe_test() -> DriverResult<()> {
    let Some(dev) = first_block_device() else {
        return Err(DriverError::NotFound);
    };
    let mut dev = dev.lock();
    let mut buf = [0u8; BLOCK_SIZE];
    dev.read_blocks(Lba(0), &mut buf)?;
    logging::info!(
        "[driver-la] virtio-blk read block0 ok, first16={:02x?}",
        &buf[..16]
    );
    Ok(())
}

/// 驱动自检：尝试完整 init_after_boot 路径。
pub fn test() {
    logging::trace!("[driver-la] test begin");
    match init_after_boot() {
        Ok(()) => {
            let _ = virtio_blk_probe_test();
        }
        Err(e) => {
            logging::warn!(
                "[driver-la] init_after_boot failed: {:?}",
                e
            );
        }
    }
    logging::trace!("[driver-la] test end");
}
