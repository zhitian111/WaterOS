#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{DriverError, DriverResult, MmioRegion};
use block::{block_device_count, register_block_device, BlockDevice, VirtioBlkDevice, BLOCK_SIZE};
use fs::devfs::active_impl as devfs_impl;
use spin::Mutex;

mod pci;

/// DTB 物理基址；为 0 时退化至硬编码 ECAM 基址。
static DTB_BASE_ADDR : AtomicUsize = AtomicUsize::new(0);
/// 成功注册为 virtio-blk 的 MMIO 窗口列表。
static VIRTIO_BLK_MMIO : Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());

pub fn init_when_boot(dtb_pa : usize) { DTB_BASE_ADDR.store(dtb_pa, Ordering::Release); }

/// 物理 RAM 上界（不包含）：loongarch64 virt 当前硬编码为 2 GiB（与 QEMU `-m
/// 2G` 及 `kernel_main` 一致）。
pub fn physical_ram_end_exclusive() -> usize {
    let _fallback = wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_END;
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb != 0 {
        if let Ok(fdt) = read_fdt() {
            let mut best_end = 0usize;
            for node in fdt.all_nodes() {
                if !node.name
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
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
    Ok(fdt)
}

/// 扫描 PCIe ECAM 总线寻找 virtio-blk 设备并注册。
pub fn init_after_boot() -> DriverResult<()> {
    for e in block::supported_devices() {
        logging::info!("[driver-la] supported-device catalog: subsystem={} name={} compatible={}",
                       e.subsystem,
                       e.name,
                       e.compatible);
    }

    let mut blk = VIRTIO_BLK_MMIO.lock();
    blk.clear();

    // 尝试 PCIe ECAM 枚举 virtio-blk 设备
    let ecam_base = pci::find_ecam_base(DTB_BASE_ADDR.load(Ordering::Acquire));
    logging::info!("[driver-la] PCIe ECAM base = {:#x}",
                   ecam_base);
    if let Some(bar0_addr) = pci::probe_virtio_blk_pci(ecam_base) {
        let region = MmioRegion { base : bar0_addr,
                                  size : 0x200 };
        match VirtioBlkDevice::from_mmio(region) {
            Ok(dev) => {
                let shared = {
                    let dev : Box<dyn BlockDevice> = Box::new(dev);
                    Arc::new(Mutex::new(dev))
                };
                let idx = register_block_device(shared);
                blk.push(region);
                logging::info!("[driver-la] registered virtio-blk #{} via PCI at BAR0={:#x}",
                               idx,
                               bar0_addr);
            }
            Err(err) => {
                logging::warn!("[driver-la] failed to init virtio-blk via PCI BAR0={:#x}: {:?}",
                               bar0_addr,
                               err);
            }
        }
    }

    let registered = block_device_count();
    logging::info!("[driver-la] block devices registered={}",
                   registered);
    if registered == 0 {
        logging::warn!("[driver-la] no block device registered; root fs may use NotMounted \
                        unless a virtio-blk is present. QEMU example: `-device \
                        virtio-blk-pci,drive=x0 -drive file=...,if=none,format=raw,id=x0`.");
    }

    // devfs 同步：填充 /dev/sys/* 视图
    let node_count = devfs_impl::refresh();
    logging::info!("[driver-la] devfs refreshed, nodes={}",
                   node_count);

    Ok(())
}

/// 对已注册的首个 virtio-blk 执行块 0 读取自检。
pub fn virtio_blk_probe_test() -> DriverResult<()> {
    let blk = VIRTIO_BLK_MMIO.lock();
    let Some(mmio) = blk.first().copied() else {
        return Err(DriverError::NotFound);
    };
    drop(blk);
    let mut dev = VirtioBlkDevice::from_mmio(mmio)?;
    let mut buf = [0u8; BLOCK_SIZE];
    dev.read_blocks(block::Lba(0), &mut buf)?;
    logging::info!("[driver-la] virtio-blk read block0 ok, first16={:02x?}",
                   &buf[..16]);
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
            logging::warn!("[driver-la] init_after_boot failed: {:?}",
                           e);
        }
    }
    logging::trace!("[driver-la] test end");
}
