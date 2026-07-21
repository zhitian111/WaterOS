//! QEMU LoongArch64 `virt` 平台驱动：PCIe ECAM 枚举 virtio-blk/net、UART 与 devfs 同步。
//!
//! 与 RISC-V OpenSBI 路径不同，块/网卡走 **VirtIO PCI** 而非 virtio-mmio DTB 扫描。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use api_v0::{DriverError, DriverResult};
use block::{
    block_device_count, first_block_device, register_block_device, Lba,
    VirtioPciProbeInfo, BLOCK_SIZE,
};
#[cfg(feature = "block-cache")]
use block::BlockCacheManager;
use character::{character_device_count, register_builtin_character_devices};
use fs::devfs::active_impl as devfs_impl;
use network::{network_device_count, register_network_device, NetworkDevice, VirtioNetPciProbeInfo};
use spin::Mutex;

mod pci;
pub mod uart;

/// DTB 物理基址；为 0 时退化至硬编码 PCI 配置空间基址。
static DTB_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
/// 成功注册为 virtio-blk 的 PCI 设备列表。
static VIRTIO_BLK_PCI: Mutex<Vec<VirtioPciProbeInfo>> = Mutex::new(Vec::new());
/// 成功注册为 virtio-net 的 PCI 设备列表。
static VIRTIO_NET_PCI: Mutex<Vec<VirtioNetPciProbeInfo>> = Mutex::new(Vec::new());
static INIT_AFTER_BOOT_DONE: AtomicBool = AtomicBool::new(false);

/// 与上层 `wateros-driver` 聚合入口的引导约定一致：保存 DTB 并初始化早期 UART。
pub fn init_when_boot(dtb_pa: usize) {
    DTB_BASE_ADDR.store(dtb_pa, Ordering::Release);
    uart::init_early_default_uart();
}

/// 物理 RAM 上界（不包含）：QEMU LoongArch64 `virt -m 1G` 的可用 RAM
/// 包含内核所在的 `0x8000_0000..0xb000_0000` 高段；内核从
/// `0x9000_0000` 启动，因此 frame allocator 的高段 fallback 必须停在
/// `0xb000_0000`，不能把中间 MMIO/空洞当作 RAM。
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
    // 回退：DTB 不可用时匹配仓库 Makefile 的 QEMU `virt -m 1G`。
    0xb000_0000
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

    let mut blk = VIRTIO_BLK_PCI.lock();
    blk.clear();
    drop(blk);
    VIRTIO_NET_PCI.lock().clear();

    // 尝试 PCIe ECAM 枚举 virtio-blk 设备。
    let config_base = pci::find_config_base(DTB_BASE_ADDR.load(Ordering::Acquire));
    log::info!(
        "[driver-la] PCI config base = {:#x}",
        config_base
    );
    match pci::probe_virtio_blk_pci(config_base) {
        Ok(Some((dev, info))) => {
            let shared = {
                #[cfg(feature = "block-cache")]
                {
                    BlockCacheManager::wrap(
                        Box::new(dev),
                        BlockCacheManager::default_config(),
                    )
                }
                #[cfg(not(feature = "block-cache"))]
                {
                    let dev: Box<dyn BlockDevice> = Box::new(dev);
                    Arc::new(Mutex::new(dev))
                }
            };
            let idx = register_block_device(shared);
            VIRTIO_BLK_PCI.lock().push(info);
            log::info!(
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
            log::warn!("[driver-la][pci] no virtio-blk device found on PCI bus 0");
        }
        Err(err) => {
            log::warn!(
                "[driver-la] failed to init virtio-blk via PCI: {:?}",
                err
            );
        }
    }

    match pci::probe_virtio_net_pci(config_base) {
        Ok(Some((dev, info))) => {
            let mac = dev.mac_address();
            let shared = {
                let dev: Box<dyn NetworkDevice> = Box::new(dev);
                Arc::new(Mutex::new(dev))
            };
            let idx = register_network_device(shared);
            VIRTIO_NET_PCI.lock().push(info);
            log::info!(
                "[driver-la] registered virtio-net #{} via PCI {}:{}.{} vendor={:#06x} \
                            device={:#06x}",
                idx,
                info.bus,
                info.device,
                info.function,
                info.vendor_id,
                info.device_id
            );
            log::info!("[driver-la] found virtio-net-pci mac={:02x?}", mac);
        }
        Ok(None) => {
            log::warn!("[driver-la][pci] no virtio-net device found on PCI bus 0");
        }
        Err(err) => {
            log::warn!(
                "[driver-la] failed to init virtio-net via PCI: {:?}",
                err
            );
        }
    }

    register_builtin_character_devices();

    let registered = block_device_count();
    let registered_net = network_device_count();
    let registered_chr = character_device_count();
    log::info!(
        "[driver-la] devices registered: block={} network={} character={}",
        registered,
        registered_net,
        registered_chr
    );
    if registered == 0 {
        log::warn!(
            "[driver-la] no block device registered; root fs may use NotMounted \
                        unless a virtio-blk is present. QEMU example: `-device \
                        virtio-blk-pci,drive=x0 -drive file=...,if=none,format=raw,id=x0`."
        );
    } else if let Err(err) = virtio_blk_probe_test() {
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

    // devfs 同步：填充 /dev/sys/* 视图
    let node_count = devfs_impl::refresh();
    log::info!(
        "[driver-la] devfs refreshed, nodes={}",
        node_count
    );
    uart::init_default_virt_uart();
    log::info!("[driver-la] QEMU LoongArch64 UART16550 ready (serial I/O)");

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
