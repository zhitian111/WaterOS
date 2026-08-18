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

use api_v0::{DriverError, DriverResult};
use block::{VirtioPciBarAllocator, VirtioPciBlkDevice, VirtioPciProbeInfo};
use network::{VirtioNetPciBarAllocator, VirtioNetPciProbeInfo, VirtioPciNetDevice};
#[cfg(feature = "display")]
use display::{VirtioGpuPciBarAllocator, VirtioGpuPciDevice, VirtioGpuPciProbeInfo};
#[cfg(feature = "input")]
use input::{VirtioInputPciBarAllocator, VirtioInputPciDevice, VirtioInputPciProbeInfo};

/// QEMU LoongArch64 virt 的默认 PCIe ECAM 配置空间基址。
const PCI_CONFIG_DEFAULT_BASE: usize = 0x2000_0000;
const PCI_MMIO_BASE: u64 = 0x4000_0000;
const PCI_NET_MMIO_BASE: u64 = 0x5000_0000;
#[cfg(feature = "display")]
const PCI_GPU_MMIO_BASE: u64 = 0x6000_0000;
#[cfg(feature = "input")]
const PCI_INPUT_MMIO_BASE: u64 = 0x7000_0000;
const PCI_MMIO_END: u64 = 0x8000_0000;

const LS7A_TOY_READ0_OFFSET: usize = 0x2c;
const LS7A_TOY_READ1_OFFSET: usize = 0x30;
const LS7A_RTC_CTRL_OFFSET: usize = 0x40;
const LS7A_RTC_REQUIRED_SIZE: usize = LS7A_RTC_CTRL_OFFSET + core::mem::size_of::<u32>();
const LS7A_RTC_CTRL_TOY_ENABLE: u32 = 1 << 11;
const LS7A_RTC_CTRL_ENABLE_OUTPUT: u32 = 1 << 8;

fn mmio_read32(base: usize, byte_offset: usize) -> u32 {
    // 寄存器地址由 DTB 区间校验过；使用 volatile 保留硬件读取语义。
    let ptr = base.wrapping_add(byte_offset) as *const u32;
    unsafe { core::ptr::read_volatile(ptr) }
}

fn mmio_write32(base: usize, byte_offset: usize, value: u32) {
    let ptr = base.wrapping_add(byte_offset) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, value) }
}

/// 从 LS7A TOY 寄存器读取 QEMU 提供的 UTC，并转换为 Unix 纳秒时间戳。
///
/// 裸内核启动没有固件替我们打开日历计数器，因此读取前需要保留控制寄存器的其他位，
/// 再启用 TOY 与输出总开关。年份在跨年瞬间可能变化，前后两次年份一致时才采用本次结果。
pub fn ls7a_rtc_realtime_ns() -> DriverResult<u64> {
    let fdt = common::dtb::read_fdt(platform::dtb_pa())?;
    let rtc = fdt
        .all_nodes()
        .find(|node| {
            common::dtb::compatible_list(node)
                .iter()
                .any(|compatible| compatible == "loongson,ls7a-rtc")
        })
        .and_then(common::dtb::first_mmio_region)
        .filter(|region| region.size >= LS7A_RTC_REQUIRED_SIZE)
        .ok_or(DriverError::NotFound)?;

    let control = mmio_read32(rtc.base, LS7A_RTC_CTRL_OFFSET);
    let enabled_control = control | LS7A_RTC_CTRL_TOY_ENABLE | LS7A_RTC_CTRL_ENABLE_OUTPUT;
    if enabled_control != control {
        mmio_write32(rtc.base, LS7A_RTC_CTRL_OFFSET, enabled_control);
    }

    // 年份可能在两次读取间跨年；最多重试三次，避免硬件异常时无限循环。
    for _ in 0..3 {
        let year_before = mmio_read32(rtc.base, LS7A_TOY_READ1_OFFSET);
        let calendar = mmio_read32(rtc.base, LS7A_TOY_READ0_OFFSET);
        let year_after = mmio_read32(rtc.base, LS7A_TOY_READ1_OFFSET);
        if year_before != year_after {
            continue;
        }

        let fields = platform::wall_clock::RtcTimeFields {
            tm_sec: ((calendar >> 4) & 0x3f) as i32,
            tm_min: ((calendar >> 10) & 0x3f) as i32,
            tm_hour: ((calendar >> 16) & 0x1f) as i32,
            tm_mday: ((calendar >> 21) & 0x1f) as i32,
            tm_mon: (((calendar >> 26) & 0x3f) as i32) - 1,
            tm_year: year_before as i32,
            ..Default::default()
        };
        if fields.tm_sec > 59
            || fields.tm_min > 59
            || fields.tm_hour > 23
            || fields.tm_mday > 31
        {
            // 保留寄存器中的非法 BCD/范围值为 I/O 错误，不生成错误时间戳。
            return Err(DriverError::IoError);
        }
        let ns = platform::wall_clock::rtc_time_to_ns(&fields)
            .map_err(|_| DriverError::IoError)?;
        return u64::try_from(ns).map_err(|_| DriverError::IoError);
    }

    Err(DriverError::IoError)
}

/// 从 DTB 中解析 `pci@*` 节点的 `reg` 段，优先寻找 QEMU LoongArch 的配置窗口；
/// 失败则回退到硬编码默认值。
pub fn find_config_base() -> usize {
    if let Ok(fdt) = common::dtb::read_fdt(platform::dtb_pa()) {
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
    PCI_CONFIG_DEFAULT_BASE
}

/// 扫描 PCI bus 0 上所有设备，定位并初始化 virtio-blk。
pub fn probe_virtio_blk_pci(
    config_base: usize,
) -> DriverResult<Option<(VirtioPciBlkDevice, VirtioPciProbeInfo)>> {
    let mut allocator = VirtioPciBarAllocator::new(PCI_MMIO_BASE, PCI_MMIO_END);
    unsafe { VirtioPciBlkDevice::probe_first_from_ecam(config_base, &mut allocator) }
}

/// 扫描 PCI bus 0 上所有设备，定位并初始化 virtio-net。
pub fn probe_virtio_net_pci(
    config_base: usize,
) -> DriverResult<Option<(VirtioPciNetDevice, VirtioNetPciProbeInfo)>> {
    let mut allocator = VirtioNetPciBarAllocator::new(PCI_NET_MMIO_BASE, PCI_MMIO_END);
    unsafe { VirtioPciNetDevice::probe_first_from_ecam(config_base, &mut allocator) }
}

/// 扫描 PCI bus 0 上所有设备，定位并初始化 virtio-gpu。
#[cfg(feature = "display")]
pub fn probe_virtio_gpu_pci(
    config_base: usize,
) -> DriverResult<Option<(VirtioGpuPciDevice, VirtioGpuPciProbeInfo)>> {
    let mut allocator = VirtioGpuPciBarAllocator::new(PCI_GPU_MMIO_BASE, PCI_MMIO_END);
    unsafe { VirtioGpuPciDevice::probe_first_from_ecam(config_base, &mut allocator) }
}

/// 扫描 PCI bus 0 上全部 virtio-input。键盘和平板是两个独立设备，不能只取首个。
#[cfg(feature = "input")]
pub fn probe_virtio_input_pci(
    config_base: usize,
) -> DriverResult<alloc::vec::Vec<(VirtioInputPciDevice, VirtioInputPciProbeInfo)>> {
    let mut allocator = VirtioInputPciBarAllocator::new(PCI_INPUT_MMIO_BASE, PCI_MMIO_END);
    unsafe { VirtioInputPciDevice::probe_all_from_ecam(config_base, &mut allocator) }
}
