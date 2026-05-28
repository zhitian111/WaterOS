//! LoongArch64 **PCIe ECAM 枚举**：从 DTB/默认值定位配置空间基址，并委托块设备
//! PCI transport 扫描 bus 0 上的 virtio-blk。
//!
//! ## 硬件假设
//!
//! - QEMU LoongArch64 `virt` 机型的 PCIe 配置空间基址为 `0x2000_0000`，配置空间使用
//!   ECAM 偏移（bus << 20 | device << 15 | function << 12 | offset）。
//! - PCI MMIO window 为 `0x4000_0000..0x8000_0000`；裸 `-kernel` 启动没有固件分配
//!   BAR，初始化 VirtIO PCI transport 前需要为 MMIO BAR 做最小分配。
//! - Root bus = 0，设备/功能枚举由 `virtio-drivers` 的 `PciRoot` 完成。
//! - VirtIO PCI BAR 不是 VirtIO-MMIO 寄存器文件，必须使用 `PciTransport` 解析
//!   vendor capabilities。

use api_v0::DriverResult;
use block::{VirtioPciBarAllocator, VirtioPciBlkDevice, VirtioPciProbeInfo};

/// QEMU LoongArch64 virt 的默认 PCIe ECAM 配置空间基址。
const PCI_CONFIG_DEFAULT_BASE: usize = 0x2000_0000;
const PCI_MMIO_BASE: u64 = 0x4000_0000;
const PCI_MMIO_END: u64 = 0x8000_0000;

/// 从 DTB 中解析 `pci@*` 节点的 `reg` 段，优先寻找 QEMU LoongArch 的配置窗口；
/// 失败则回退到硬编码默认值。
pub fn find_config_base(dtb_pa: usize) -> usize {
    if dtb_pa != 0 {
        if let Ok(fdt) = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) } {
            for node in fdt.all_nodes() {
                if !node
                    .name
                    .starts_with("pci")
                {
                    continue;
                }
                let Some(mut regions) = node.reg() else {
                    continue;
                };
                while let Some(region) = regions.next() {
                    let base = region.starting_address as usize;
                    if base == PCI_CONFIG_DEFAULT_BASE {
                        return base;
                    }
                }
            }
        }
    }
    PCI_CONFIG_DEFAULT_BASE
}

/// 扫描 PCI bus 0 上所有设备，定位并初始化 virtio-blk。
pub fn probe_virtio_blk_pci(
    config_base: usize,
) -> DriverResult<Option<(VirtioPciBlkDevice, VirtioPciProbeInfo)>> {
    let mut allocator = VirtioPciBarAllocator::new(PCI_MMIO_BASE, PCI_MMIO_END);
    unsafe { VirtioPciBlkDevice::probe_first_from_ecam(config_base, &mut allocator) }
}
