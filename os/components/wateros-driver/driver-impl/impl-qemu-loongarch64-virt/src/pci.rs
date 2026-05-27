//! LoongArch64 **PCIe ECAM 枚举**：在 ECAM 配置空间扫描 bus 0 的所有设备，
//! 寻找 virtio-blk 的 BAR0 MMIO 基址。
//!
//! ## 硬件假设
//!
//! - QEMU LoongArch64 `virt` 机型的 PCIe ECAM 基址 `0x1A00_0000`（与 QEMU 源码
//!   `hw/loongarch/virt.c` 中硬编码一致）。若固件提供了 FDT/ACPI `pci@*`
//!   描述，可通过 DTB 解析覆盖。
//! - Root bus = 0，至多扫描 device 0..31, function 0。
//! - 配置空间按 PCI ECAM 规范组织： `ecam_base + (bus << 20 | device << 15 |
//!   function << 12) + offset`
//! - VirtIO 设备 ID 同时匹配 modern (`0x1042`) 和 transitional (`0x1001`)。
//! - BAR0 为 MMIO 型（`bit0 == 0`）且包含完整的 VirtIO MMIO 寄存器文件。

const PCI_VENDOR_VIRTIO : u16 = 0x1AF4;
const PCI_DEVICE_VIRTIO_BLK_MODERN : u16 = 0x1042;
const PCI_DEVICE_VIRTIO_BLK_TRANS : u16 = 0x1001;

/// QEMU LoongArch64 virt 的默认 PCIe ECAM 基址。
const ECAM_DEFAULT_BASE : usize = 0x1A00_0000;

/// 从 DTB 中解析 `pci@*` 节点的 `reg` 第一段作为 ECAM 基址；失败则回退到
/// 硬编码默认值。
pub fn find_ecam_base(dtb_pa : usize) -> usize {
    if dtb_pa != 0 {
        if let Ok(fdt) = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) } {
            for node in fdt.all_nodes() {
                if !node.name
                        .starts_with("pci")
                {
                    continue;
                }
                let Some(mut regions) = node.reg() else {
                    continue;
                };
                if let Some(region) = regions.next() {
                    let ecam = region.starting_address as usize;
                    if ecam > 0 {
                        return ecam;
                    }
                }
            }
        }
    }
    ECAM_DEFAULT_BASE
}

/// ECAM 配置空间 32 位读。
#[inline]
fn ecam_read32(ecam_base : usize, bus : u8, dev : u8, func : u8, offset : u16) -> u32 {
    let addr = ecam_base + ((bus as usize) << 20) |
               ((dev as usize) << 15) |
               ((func as usize) << 12) |
               (offset as usize & 0xFFF);
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// ECAM 配置空间 16 位读。
#[inline]
fn ecam_read16(ecam_base : usize, bus : u8, dev : u8, func : u8, offset : u16) -> u16 {
    let addr = ecam_base + ((bus as usize) << 20) |
               ((dev as usize) << 15) |
               ((func as usize) << 12) |
               (offset as usize & 0xFFF);
    (unsafe { core::ptr::read_volatile(addr as *const u32) } & 0xFFFF) as u16
}

/// 在指定 bus/dev/func 上读 vendor_id（config offset 0）。
fn pci_vendor_id(ecam_base : usize, bus : u8, dev : u8, func : u8) -> u16 {
    ecam_read16(ecam_base, bus, dev, func, 0x00)
}

/// 在指定 bus/dev/func 上读 device_id（config offset 2）。
fn pci_device_id(ecam_base : usize, bus : u8, dev : u8, func : u8) -> u16 {
    ecam_read16(ecam_base, bus, dev, func, 0x02)
}

/// 读取 BAR0（config offset 0x10）的 32 位值，并验证其为 MMIO 类型。
fn pci_bar0(ecam_base : usize, bus : u8, dev : u8, func : u8) -> Option<usize> {
    let bar = ecam_read32(ecam_base, bus, dev, func, 0x10);
    if bar == 0 || bar == 0xFFFF_FFFF {
        return None;
    }
    // bit0 = 0 表示 MMIO（非 IO）
    if (bar & 1) != 0 {
        return None;
    }
    // 清除低 4 位（BAR 属性/大小编码位）
    Some((bar & !0xF) as usize)
}

/// 扫描 PCI bus 0 上所有设备，定位 virtio-blk 并返回其 BAR0 MMIO 地址。
pub fn probe_virtio_blk_pci(ecam_base : usize) -> Option<usize> {
    for dev in 0u8..32u8 {
        let vendor = pci_vendor_id(ecam_base, 0, dev, 0);
        // vendor_id == 0xFFFF 表示设备不存在
        if vendor == 0xFFFF {
            continue;
        }
        if vendor != PCI_VENDOR_VIRTIO {
            logging::trace!("[driver-la][pci] bus=0 dev={} vendor={:#06x} (skip)",
                            dev,
                            vendor);
            continue;
        }
        let device = pci_device_id(ecam_base, 0, dev, 0);
        logging::info!("[driver-la][pci] bus=0 dev={} vendor={:#06x} device={:#06x}",
                       dev,
                       vendor,
                       device);
        if device != PCI_DEVICE_VIRTIO_BLK_MODERN && device != PCI_DEVICE_VIRTIO_BLK_TRANS {
            logging::info!("[driver-la][pci] bus=0 dev={} is virtio but not blk (device={:#06x})",
                           dev,
                           device);
            continue;
        }
        let Some(bar0) = pci_bar0(ecam_base, 0, dev, 0) else {
            logging::warn!("[driver-la][pci] bus=0 dev={} virtio-blk has no valid MMIO BAR0",
                           dev);
            continue;
        };
        logging::info!("[driver-la][pci] found virtio-blk bus=0 dev={} BAR0={:#x}",
                       dev,
                       bar0);
        return Some(bar0);
    }
    logging::warn!("[driver-la][pci] no virtio-blk device found on PCI bus 0");
    None
}
