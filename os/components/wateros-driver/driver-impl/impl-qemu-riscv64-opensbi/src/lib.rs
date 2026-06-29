//! QEMU `virt` 机器、RISC-V64、OpenSBI 环境下的设备枚举与 virtio-blk 绑定实现。
//!
//! 依赖引导期传入的 DTB 物理指针；[`physical_ram_end_exclusive`] 从 `memory@*` 推断 RAM 顶端，失败时回退到 `wateros_base_config`。后续若支持多内存条或非连续布局，应在此集中调整解析策略。

#![no_std]
extern crate alloc;

pub mod uart;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use api_v0::{DeviceInfo, DeviceType, DriverError, DriverResult, IrqLine, MmioRegion};
use block::{
    block_device_count, block_subsystem_claims_device, register_block_device, BlockDevice, Lba,
    VirtioBlkDevice, BLOCK_SIZE,
};
#[cfg(feature = "block-cache")]
use block::BlockCacheManager;
use network::{
    network_device_count, network_subsystem_claims_device, register_network_device,
    NetworkDevice, VirtioNetDevice,
};
use character::{
    character_device_count, character_subsystem_claims_device, is_uart_compatible,
    register_builtin_character_devices, register_character_device, CharacterDevice,
    SerialPortCharacterDevice,
};
use fdt::Fdt;
use fs::devfs::active_impl as devfs_impl;
use spin::Mutex;
use uart::QemuVirtUart16550;

// DTB 物理基址；为 0 时 read_fdt 返回 NotFound（尚未 boot 或未调用 init_when_boot）。
static DTB_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
// 最近一次 scan_device_info 填充的节点摘要表。
static DEVICE_INFOS: Mutex<Vec<DeviceInfo>> = Mutex::new(Vec::new());
// 成功注册为 virtio-blk 的 MMIO 窗口列表（供自检读取块 0）。
static VIRTIO_BLK_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());
// 成功注册为 virtio-net 的 MMIO 窗口列表。
static VIRTIO_NET_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());
static INIT_AFTER_BOOT_DONE: AtomicBool = AtomicBool::new(false);

/// 与上层 `wateros-driver` 聚合入口的引导约定一致：仅保存 `dtb_pa`。
#[inline]
pub fn init_when_boot(dtb_pa: usize) {
    DTB_BASE_ADDR.store(dtb_pa, Ordering::Release);
}

/// 物理 RAM 上界（不包含）：优先解析 DTB `memory@*` 的 `reg`；失败时用 `wateros-base-config` 回退值。
#[inline]
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

// `unsafe`：`dtb_pa` 指向的 DTB 在内核存活期内常驻且布局合法；返回的 `Fdt` 仅在本 crate 扫描路径中使用。
fn read_fdt() -> DriverResult<Fdt<'static>> {
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb == 0 {
        return Err(DriverError::NotFound);
    }
    let fdt = unsafe { Fdt::from_ptr(dtb as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
    Ok(fdt)
}

// DTB 属性值为大端；`offset` 须对齐到 4 字节边界（此处由调用方保证长度）。
fn read_be_u32(raw: &[u8], offset: usize) -> Option<u32> {
    let bytes = raw.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

// 取节点 `reg` 的第一段作为 MMIO 窗口；多段设备当前仅使用首段（virtio-mmio 在 virt 上通常一段即可）。
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

// 仅覆盖「单 cell 中断号 + 可选 interrupt-parent」形态；PLIC/GPIO 复用等复杂描述返回 `None` 而非误解析。
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

// `compatible` 为以 `NUL` 分隔的 C 字符串序列；非法 UTF-8 片段丢弃。
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

// 与块子系统 `supported_devices` 中 `virtio,mmio` 字符串精确一致才视为 virtio-mmio 节点。
fn is_virtio_mmio_compatible(compatibles: &[String]) -> bool {
    compatibles.iter().any(|c| c.as_str() == "virtio,mmio")
}

// `word_offset` 为 u32 字偏移，与 VirtIO-MMIO 寄存器布局一致；访问须落在已映射的物理窗口内。
fn mmio_read32(base: usize, word_offset: usize) -> u32 {
    let ptr = (base as *const u32).wrapping_add(word_offset);
    unsafe { core::ptr::read_volatile(ptr) }
}

// 魔数 0x74726976 即小端 "virt"；device id 遵循 VirtIO 规范（2=block，1=network）。
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

// 生成 devfs 侧稳定路径片段；将 `@`/`/` 替换为 `_` 避免路径分隔歧义。
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
            } else if is_uart_compatible(&compatibles) {
                dtype = DeviceType::Character;
            }
        } else if is_uart_compatible(&compatibles) {
            dtype = DeviceType::Character;
        }

        if is_virtio_mmio_compatible(&compatibles) {
            match mmio {
                Some(m) => {
                    let magic = mmio_read32(m.base, 0);
                    let device_id = mmio_read32(m.base, 2);
                    log::info!(
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
                    log::warn!(
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

/// 在关锁临界区内只读访问设备信息快照。
pub fn with_device_infos<R>(f: impl FnOnce(&[DeviceInfo]) -> R) -> R {
    let infos = DEVICE_INFOS.lock();
    f(infos.as_slice())
}

// 在已扫描的 `DEVICE_INFOS` 上尝试实例化 virtio-blk 与 virtio-net；失败或未声明的路径记入列表供 devfs 标注。
fn probe_virtio_blk_and_collect_unsupported() -> Vec<String> {
    let infos_snapshot: Vec<DeviceInfo> = DEVICE_INFOS.lock().clone();
    VIRTIO_BLK_MMIO.lock().clear();
    VIRTIO_NET_MMIO.lock().clear();

    let mut unsupported = Vec::new();
    let mut blk_regions = Vec::new();
    let mut net_regions = Vec::new();

    for info in infos_snapshot.iter() {
        if !is_virtio_mmio_compatible(&info.compatibles) {
            continue;
        }

        let path = sys_dev_path_for_dtb_node(&info.node_name);

        let Some(mmio) = info.mmio else {
            unsupported.push(path);
            continue;
        };

        let claimed_by_block = block_subsystem_claims_device(&info.compatibles, info.device_type);
        let claimed_by_network =
            network_subsystem_claims_device(&info.compatibles, info.device_type);

        if claimed_by_block && info.device_type == DeviceType::Block {
            match VirtioBlkDevice::from_mmio(mmio) {
                Ok(dev) => {
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
                    blk_regions.push(mmio);
                    log::info!("[driver] registered virtio-blk #{}", idx);
                    log::info!(
                        "[driver] found virtio-blk: node={} base={:#x} size={:#x}",
                        info.node_name,
                        mmio.base,
                        mmio.size
                    );
                }
                Err(err) => {
                    log::warn!(
                        "[driver] failed to init virtio-blk at base={:#x}: {:?}",
                        mmio.base,
                        err
                    );
                    unsupported.push(path);
                }
            }
        } else if claimed_by_network && info.device_type == DeviceType::Network {
            match VirtioNetDevice::from_mmio(mmio) {
                Ok(dev) => {
                    let mac = dev.mac_address();
                    let idx = register_network_device(Arc::new(Mutex::new(Box::new(dev))));
                    net_regions.push(mmio);
                    log::info!("[driver] registered virtio-net #{}", idx);
                    log::info!(
                        "[driver] found virtio-net: node={} mac={:02x?} base={:#x} size={:#x}",
                        info.node_name,
                        mac,
                        mmio.base,
                        mmio.size
                    );
                }
                Err(err) => {
                    log::warn!(
                        "[driver] failed to init virtio-net at base={:#x}: {:?}",
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
    *VIRTIO_BLK_MMIO.lock() = blk_regions;
    *VIRTIO_NET_MMIO.lock() = net_regions;
    unsupported
}

fn register_uart_character_device(base: usize) -> usize {
    let mut u = QemuVirtUart16550::new(base);
    u.init_minimal();
    let shared: character::SharedCharacterDevice = Arc::new(Mutex::new(
        Box::new(SerialPortCharacterDevice::new(u)) as Box<dyn CharacterDevice>,
    ));
    register_character_device(shared)
}

/// 绑定 DTB 中的 UART 字符设备；若无匹配则回退到 QEMU virt 默认 UART0。
fn probe_character_devices() {
    let uart_bases: Vec<usize> = {
        let infos = DEVICE_INFOS.lock();
        infos
            .iter()
            .filter(|info| {
                character_subsystem_claims_device(&info.compatibles, info.device_type)
            })
            .filter_map(|info| {
                if let Some(mmio) = info.mmio {
                    Some((mmio.base, info.node_name.clone()))
                } else {
                    log::warn!(
                        "[driver] dtb uart: node={} has no MMIO region",
                        info.node_name
                    );
                    None
                }
            })
            .map(|(base, _)| base)
            .collect()
    };

    for (idx, base) in uart_bases.iter().enumerate() {
        let chr_idx = register_uart_character_device(*base);
        log::info!(
            "[driver] registered character #{} (uart base={:#x}, dtb #{})",
            chr_idx,
            base,
            idx
        );
    }

    if character_device_count() == 0 {
        let idx = register_uart_character_device(QemuVirtUart16550::qemu_virt_default().base);
        log::info!(
            "[driver] registered character #{} (fallback virt uart0 base={:#x})",
            idx,
            QemuVirtUart16550::qemu_virt_default().base
        );
    }

    register_builtin_character_devices();
    log::info!(
        "[driver] character devices registered: count={}",
        character_device_count()
    );
}

// 将 DTB 中未能绑定的 virtio 节点路径同步给用户态可见的 devfs 视图（具体语义由 devfs impl 定义）。
fn sync_devfs(unsupported_paths: Vec<String>) {
    devfs_impl::set_dt_unsupported_paths(unsupported_paths);
    let node_count = devfs_impl::refresh();
    log::info!("[driver] devfs refreshed, nodes={}", node_count);
}

// 自检日志：依赖 `logging` 级别；不改变驱动状态。
fn dump_device_and_devfs_info() {
    let infos = DEVICE_INFOS.lock();
    for (idx, info) in infos.iter().enumerate() {
        log::info!(
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
        log::info!(
            "[driver][test] devfs-node#{} path={} type={:?}",
            idx,
            node.path,
            node.node_type
        );
    }

    let root_path = devfs_impl::default_root_block_path();
    log::info!("[driver][test] devfs default root path={:?}", root_path);
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
    log::info!("[driver] virtio-blk read block0 ok, first16={:02x?}", &buf[..16]);
    Ok(())
}

/// DTB 扫描、virtio-blk / virtio-net 注册与 devfs 同步的完整 bring-up 路径；成功返回后设备表可能仍为空。
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

    let count = scan_device_info()?;
    log::trace!("[driver] dtb scan done, devices={}", count);
    probe_character_devices();
    let unsupported = probe_virtio_blk_and_collect_unsupported();
    let registered_blk = block_device_count();
    let registered_net = network_device_count();
    let registered_chr = character_device_count();
    log::info!(
        "[driver] devices registered: block={} network={} character={}",
        registered_blk,
        registered_net,
        registered_chr
    );
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
    sync_devfs(unsupported);
    log::info!("[driver] QEMU virt UART0 MMIO ready (serial I/O)");
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
    dump_device_and_devfs_info();
    let _ = virtio_blk_probe_test();
    log::trace!("[driver-impl-qemu] test end");
}
