//! QEMU `virt` 机器、RISC-V64、OpenSBI 环境下的设备枚举与 virtio-blk 绑定实现。
//!
//! 依赖引导期传入的 DTB 物理指针；[`physical_ram_end_exclusive`] 从 `memory@*` 推断 RAM 顶端，失败时回退到 `wateros_base_config`。后续若支持多内存条或非连续布局，应在此集中调整解析策略。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{DeviceInfo, DeviceType, DriverError, DriverResult, IrqLine, MmioRegion};
use block::{
    block_device_count, block_subsystem_claims_device, register_block_device, BlockDevice, Lba,
    VirtioBlkDevice, BLOCK_SIZE,
};
use fdt::Fdt;
use fs::devfs::active_impl as devfs_impl;
use spin::Mutex;

// DTB 物理基址；为 0 时 read_fdt 返回 NotFound（尚未 boot 或未调用 init_when_boot）。
static DTB_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
// 最近一次 scan_device_info 填充的节点摘要表。
static DEVICE_INFOS: Mutex<Vec<DeviceInfo>> = Mutex::new(Vec::new());
// 成功注册为 virtio-blk 的 MMIO 窗口列表（供自检读取块 0）。
static VIRTIO_BLK_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());

/// 与上层 `wateros-driver` 聚合入口的引导约定一致：仅保存 `dtb_pa`。
pub fn init_when_boot(dtb_pa: usize) {
    DTB_BASE_ADDR.store(dtb_pa, Ordering::Release);
}

/// 物理 RAM 上界（不包含）：优先解析 DTB `memory@*` 的 `reg`；失败时用 `wateros-base-config` 回退值。
pub fn physical_ram_end_exclusive() -> usize {
    use wateros_base_config::mm::QEMU_VIRT_PHYS_RAM_END as FALLBACK;
    let Ok(fdt) = read_fdt() else {
        return FALLBACK;
    };
    let mut best_end = 0usize;
    for node in fdt.all_nodes() {
        // 与 Linux/QEMU virt 常见命名一致；非规范强制，属当前 bring-up 假设。
        if !node.name.starts_with("memory") {
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
            // 忽略低于 DRAM 典型起点的区域，避免误选保留映射。
            if end > base && base >= 0x8000_0000 && end > best_end {
                best_end = end;
            }
        }
    }
    if best_end > 0x8000_0000 {
        best_end
    } else {
        FALLBACK
    }
}

fn read_fdt() -> DriverResult<Fdt<'static>> {
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb == 0 {
        return Err(DriverError::NotFound);
    }
    let fdt = unsafe { Fdt::from_ptr(dtb as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
    Ok(fdt)
}

fn read_be_u32(raw: &[u8], offset: usize) -> Option<u32> {
    let bytes = raw.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn first_mmio_region(node: fdt::node::FdtNode<'_, '_>) -> Option<MmioRegion> {
    let mut regions = node.reg()?;
    let region = regions.next()?;
    let base = region.starting_address as usize;
    let size = region.size?;
    if size == 0 {
        return None;
    }
    Some(MmioRegion { base, size })
}

fn parse_irq(node: &fdt::node::FdtNode<'_, '_>) -> Option<IrqLine> {
    let irq = node.property("interrupts")?.value;
    let irq_num = read_be_u32(irq, 0)?;
    let parent = node
        .property("interrupt-parent")
        .and_then(|p| read_be_u32(p.value, 0));
    Some(IrqLine {
        irq: irq_num,
        parent,
    })
}

fn compatible_list(node: &fdt::node::FdtNode<'_, '_>) -> Vec<String> {
    let mut list = Vec::new();
    let Some(raw) = node.property("compatible").map(|p| p.value) else {
        return list;
    };
    for item in raw.split(|b| *b == 0) {
        if item.is_empty() {
            continue;
        }
        if let Ok(text) = core::str::from_utf8(item) {
            list.push(String::from(text));
        }
    }
    list
}

fn is_virtio_mmio_compatible(compatibles: &[String]) -> bool {
    compatibles.iter().any(|c| c.as_str() == "virtio,mmio")
}

fn mmio_read32(base: usize, word_offset: usize) -> u32 {
    let ptr = (base as *const u32).wrapping_add(word_offset);
    unsafe { core::ptr::read_volatile(ptr) }
}

fn probe_virtio_device_type(mmio: MmioRegion) -> DeviceType {
    let magic = mmio_read32(mmio.base, 0);
    let device_id = mmio_read32(mmio.base, 2);
    if magic != 0x74726976 {
        return DeviceType::Unknown;
    }
    match device_id {
        2 => DeviceType::Block,
        1 => DeviceType::Network,
        _ => DeviceType::Unknown,
    }
}

fn sys_dev_path_for_dtb_node(node_name: &str) -> String {
    let safe = node_name.replace('@', "_").replace('/', "_");
    alloc::format!("/dev/sys/{}", safe)
}

/// 遍历 DTB 全部节点，重建全局设备信息表（先清空）；返回表中条目数。
pub fn scan_device_info() -> DriverResult<usize> {
    let fdt = read_fdt()?;
    let mut devices = DEVICE_INFOS.lock();
    devices.clear();

    for node in fdt.all_nodes() {
        let compatibles = compatible_list(&node);
        if compatibles.is_empty() {
            continue;
        }
        let compatible = compatibles[0].clone();
        let mmio = first_mmio_region(node);
        let mut dtype = DeviceType::Unknown;
        if let Some(region) = mmio {
            if is_virtio_mmio_compatible(&compatibles) {
                dtype = probe_virtio_device_type(region);
            }
        }

        if is_virtio_mmio_compatible(&compatibles) {
            match mmio {
                Some(m) => {
                    let magic = mmio_read32(m.base, 0);
                    let device_id = mmio_read32(m.base, 2);
                    logging::info!(
                        "[driver] dtb virtio-mmio: node={} mmio=base {:#x} size {:#x} magic={:#x} device_id={} -> {:?}",
                        node.name,
                        m.base,
                        m.size,
                        magic,
                        device_id,
                        dtype
                    );
                }
                None => {
                    logging::warn!(
                        "[driver] dtb virtio-mmio: node={} has no MMIO region (check FdtNode::reg / #address-cells)",
                        node.name
                    );
                }
            }
        }

        devices.push(DeviceInfo {
            node_name: String::from(node.name),
            compatible,
            compatibles,
            device_type: dtype,
            mmio,
            irq: parse_irq(&node),
        });
    }
    Ok(devices.len())
}

/// 指向全局设备信息表的静态互斥锁；调用方自行加锁与生命周期约束。
pub fn device_infos() -> &'static Mutex<Vec<DeviceInfo>> {
    &DEVICE_INFOS
}

fn probe_virtio_blk_and_collect_unsupported() -> Vec<String> {
    let infos = DEVICE_INFOS.lock();
    let mut blk = VIRTIO_BLK_MMIO.lock();
    blk.clear();
    let mut unsupported = Vec::new();

    for info in infos.iter() {
        if !is_virtio_mmio_compatible(&info.compatibles) {
            continue;
        }

        let path = sys_dev_path_for_dtb_node(&info.node_name);

        let Some(mmio) = info.mmio else {
            unsupported.push(path);
            continue;
        };

        let claimed = block_subsystem_claims_device(&info.compatibles, info.device_type);

        if claimed && info.device_type == DeviceType::Block {
            match VirtioBlkDevice::from_mmio(mmio) {
                Ok(dev) => {
                    let idx = register_block_device(Arc::new(Mutex::new(Box::new(dev))));
                    blk.push(mmio);
                    logging::info!("[driver] registered virtio-blk #{}", idx);
                    logging::info!(
                        "[driver] found virtio-blk: node={} base={:#x} size={:#x}",
                        info.node_name,
                        mmio.base,
                        mmio.size
                    );
                }
                Err(err) => {
                    logging::warn!(
                        "[driver] failed to init virtio-blk at base={:#x}: {:?}",
                        mmio.base,
                        err
                    );
                    unsupported.push(path);
                }
            }
        } else {
            unsupported.push(path);
        }
    }
    unsupported
}

fn sync_devfs(unsupported_paths: Vec<String>) {
    devfs_impl::set_dt_unsupported_paths(unsupported_paths);
    let node_count = devfs_impl::refresh();
    logging::info!("[driver] devfs refreshed, nodes={}", node_count);
}

fn dump_device_and_devfs_info() {
    let infos = DEVICE_INFOS.lock();
    for (idx, info) in infos.iter().enumerate() {
        logging::info!(
            "[driver][test] dev#{} node={} compatible={} compatibles={:?} type={:?} mmio={:?} irq={:?}",
            idx,
            info.node_name,
            info.compatible,
            info.compatibles,
            info.device_type,
            info.mmio,
            info.irq
        );
    }
    drop(infos);

    let dev_nodes = devfs_impl::list_nodes();
    for (idx, node) in dev_nodes.iter().enumerate() {
        logging::info!(
            "[driver][test] devfs-node#{} path={} type={:?}",
            idx,
            node.path,
            node.node_type
        );
    }

    let root_path = devfs_impl::default_root_block_path();
    logging::info!("[driver][test] devfs default root path={:?}", root_path);
}

/// 对已注册的首个 virtio-blk 执行块 0 读取自检；无设备时 [`DriverError::NotFound`]。
pub fn virtio_blk_probe_test() -> DriverResult<()> {
    let blk = VIRTIO_BLK_MMIO.lock();
    let Some(mmio) = blk.first().copied() else {
        return Err(DriverError::NotFound);
    };
    drop(blk);
    let mut dev = VirtioBlkDevice::from_mmio(mmio)?;
    let mut buf = [0u8; BLOCK_SIZE];
    dev.read_blocks(Lba(0), &mut buf)?;
    logging::info!("[driver] virtio-blk read block0 ok, first16={:02x?}", &buf[..16]);
    Ok(())
}

pub fn init_after_boot() -> DriverResult<()> {
    for e in block::supported_devices() {
        logging::info!(
            "[driver] supported-device catalog: subsystem={} name={} compatible={}",
            e.subsystem,
            e.name,
            e.compatible
        );
    }

    let count = scan_device_info()?;
    logging::info!("[driver] dtb scan done, devices={}", count);
    let unsupported = probe_virtio_blk_and_collect_unsupported();
    let registered = block_device_count();
    logging::info!("[driver] block devices registered={}", registered);
    if registered == 0 {
        logging::warn!(
            "[driver] no block device registered; root fs may use NotMounted unless a virtio-blk is present. \
             QEMU virt example: `-drive file=...,if=none,id=d0 -device virtio-blk-device,drive=d0`."
        );
    }
    sync_devfs(unsupported);
    Ok(())
}

/// 驱动自检：尝试完整 `init_after_boot` 路径并打印设备与 devfs 摘要。
pub fn test() {
    logging::trace!("[driver-impl-qemu] test begin");
    match init_after_boot() {
        Ok(()) => {
            dump_device_and_devfs_info();
            let _ = virtio_blk_probe_test();
        }
        Err(e) => {
            logging::warn!("[driver-impl-qemu] init_after_boot failed: {:?}", e);
        }
    }
    logging::trace!("[driver-impl-qemu] test end");
}
