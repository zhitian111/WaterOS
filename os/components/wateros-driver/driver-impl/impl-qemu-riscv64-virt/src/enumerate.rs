//! DTB 设备表扫描与平台设备探测（transport: virtio-mmio）。

use alloc::{string::String, vec::Vec};

use api_v0::{DeviceInfo, DeviceType, DriverError, DriverResult, MmioRegion};
use character::is_uart_compatible;
use common::dtb;
use spin::Mutex;

/// 最近一次 [`scan_device_info`] 填充的节点摘要表；供注册与诊断只读访问。
pub(crate) static DEVICE_INFOS: Mutex<Vec<DeviceInfo>> = Mutex::new(Vec::new());

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
        16 => DeviceType::Display,
        18 => DeviceType::Input,
        _ => DeviceType::Unknown,
    }
}

/// 生成 devfs 侧稳定路径片段；将 `@`/`/` 替换为 `_` 避免路径分隔歧义。
pub(crate) fn sys_dev_path_for_dtb_node(node_name: &str) -> String {
    let safe = node_name.replace('@', "_").replace('/', "_");
    alloc::format!("/dev/sys/{}", safe)
}

/// 遍历 DTB 全部节点，重建全局设备信息表（先清空）；返回表中条目数。
pub fn scan_device_info() -> DriverResult<usize> {
    let fdt = dtb::read_fdt(platform::dtb_pa())?;
    let mut devices = DEVICE_INFOS.lock();
    devices.clear();

    for node in fdt.all_nodes() {
        let compatibles = dtb::compatible_list(&node);
        if compatibles.is_empty() {
            continue;
        }
        let compatible = compatibles[0].clone();
        let mmio = dtb::first_mmio_region(node);
        let mut dtype = DeviceType::Unknown;
        if let Some(region) = mmio {
            if dtb::is_virtio_mmio_compatible(&compatibles) {
                dtype = probe_virtio_device_type(region);
            } else if is_uart_compatible(&compatibles) {
                dtype = DeviceType::Character;
            }
        } else if is_uart_compatible(&compatibles) {
            dtype = DeviceType::Character;
        }

        if dtb::is_virtio_mmio_compatible(&compatibles) {
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
            irq: dtb::parse_irq(&node),
        });
    }
    Ok(devices.len())
}

/// 在关锁临界区内只读访问设备信息快照。
pub fn with_device_infos<R>(f: impl FnOnce(&[DeviceInfo]) -> R) -> R {
    let infos = DEVICE_INFOS.lock();
    f(infos.as_slice())
}

/// 读取 QEMU virt 的 Goldfish RTC 当前 UTC 纳秒值。
///
/// 读取低 32 位会把同一时刻的高 32 位锁存到相邻寄存器，因此顺序必须是 low、high。
pub fn goldfish_rtc_realtime_ns() -> DriverResult<u64> {
    let infos = DEVICE_INFOS.lock();
    let rtc = infos.iter()
                   .find(|info| {
                       info.compatibles
                           .iter()
                           .any(|compatible| compatible == "google,goldfish-rtc")
                   })
                   .and_then(|info| info.mmio)
                   .filter(|region| region.size >= 8)
                   .ok_or(DriverError::NotFound)?;
    let low = u64::from(mmio_read32(rtc.base, 0));
    let high = u64::from(mmio_read32(rtc.base, 1));
    let ns = (high << 32) | low;
    if ns == 0 {
        return Err(DriverError::IoError);
    }
    Ok(ns)
}
