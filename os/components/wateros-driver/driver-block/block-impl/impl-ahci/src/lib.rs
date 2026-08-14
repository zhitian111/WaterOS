//! Loongson 2K1000 AHCI/SATA 块设备（polled PIO，无中断）。
//!
//! 复用 `isomorphic_drivers::block::ahci::AHCI` 与 robigalia `pci` 的
//! MemoryMapped fork；本 crate 只提供 DMA provider、PCI 扫描与 [`BlockDevice`] 适配。

#![no_std]
extern crate alloc;

#[cfg(feature = "dma")]
use alloc::vec::Vec;
use block::BLOCK_SIZE;
#[cfg(feature = "dma")]
use block::{BlockDevice, DriverError, DriverResult, Lba};
#[cfg(feature = "dma")]
use isomorphic_drivers::provider::Provider;

/// Loongson 2K1000 PCIe 配置空间（ECAM/CAM）基址（BSP/参考实现事实）。
pub const PCI_CONFIG_BASE : usize = 0xfe_0000_0000;
/// ATA 大容量存储设备类（class=0x01）下的 SATA 控制器子类（subclass=0x06）。
pub const PCI_CLASS_MASS_STORAGE : u8 = 0x01;
pub const PCI_SUBCLASS_SATA : u8 = 0x06;

/// 纯函数：判定 class/subclass 是否为 SATA 大容量存储控制器（便于 host 单测）。
pub const fn is_sata_mass_storage_class(class_code : u8, subclass : u8) -> bool {
    class_code == PCI_CLASS_MASS_STORAGE && subclass == PCI_SUBCLASS_SATA
}

/// buf 字节数 → 512B 扇区数（纯函数，便于 host 单测）。
pub const fn sector_count_for(byte_len : usize) -> Option<usize> {
    if byte_len % BLOCK_SIZE != 0 {
        None
    } else {
        Some(byte_len / BLOCK_SIZE)
    }
}

/// DMA 请求字节数 → 所需页数（按页向上取整；host 单测覆盖对齐/溢出）。
pub const fn dma_pages_for(byte_len : usize) -> Option<usize> {
    const PAGE : usize = 4096;
    if byte_len == 0 {
        return None;
    }
    match byte_len.checked_add(PAGE - 1) {
        Some(value) => Some(value / PAGE),
        None => None,
    }
}

/// 扫描 PCI 配置空间，返回首个 SATA 控制器的 BAR0（基址、长度）。
///
/// 该路径访问真实 MMIO，不做 host 单测；class 匹配逻辑由
/// [`is_sata_mass_storage_class`] 单独测试。
pub fn find_ahci_bar() -> Option<(usize, usize)> {
    for dev in unsafe {
        pci::scan_bus(&UnusedPort, pci::CSpaceAccessMethod::MemoryMapped, PCI_CONFIG_BASE)
    } {
        if !is_sata_mass_storage_class(dev.id.class, dev.id.subclass) {
            continue;
        }
        if let Some(pci::BAR::Memory(pa, len, _, _)) = dev.bars[0] {
            if pa != 0 {
                log::info!("[ahci] found SATA controller bar0={:#x} len={:#x}",
                           pa,
                           len);
                return Some((pa as usize, len as usize));
            }
        }
    }
    None
}

/// 在 2K1000 PCIe ECAM 上枚举 AHCI 控制器并注册为块设备（需要 `dma` feature）。
#[cfg(feature = "dma")]
pub fn init() -> DriverResult<usize> {
    use alloc::{boxed::Box, sync::Arc};
    use block::{register_block_device, SharedBlockDevice};
    use spin::Mutex;

    let (base, size) = find_ahci_bar().ok_or(DriverError::NotFound)?;
    let sata = SataBlock::new(base, size)?;
    let shared : SharedBlockDevice = Arc::new(Mutex::new(Box::new(sata)));
    Ok(register_block_device(shared))
}

/// 为 AHCI 提供物理连续 DMA 页（栈式帧分配器按 PPN 递减分配）。
#[cfg(feature = "dma")]
pub struct FrameProvider;

#[cfg(feature = "dma")]
impl Provider for FrameProvider {
    const PAGE_SIZE : usize = 4096;

    fn alloc_dma(size : usize) -> (usize, usize) {
        let Some(pages) = dma_pages_for(size) else {
            return (0, 0);
        };
        let mut ppns = Vec::with_capacity(pages);
        for _ in 0..pages {
            match frame_alloctor::frame_alloc_result() {
                Ok(ppn) => ppns.push(ppn),
                Err(_) => {
                    release(&ppns);
                    return (0, 0);
                }
            }
        }
        // 栈式分配器按物理页号递减给出连续页；校验相邻 PPN。
        if !ppns.windows(2)
                .all(|pair| pair[0].0 == pair[1].0 + 1)
        {
            release(&ppns);
            return (0, 0);
        }
        let Some(base) = ppns[pages - 1].0.checked_mul(Self::PAGE_SIZE) else {
            release(&ppns);
            return (0, 0);
        };
        unsafe {
            core::ptr::write_bytes(base as *mut u8, 0, size);
        }
        // 恒等映射下 va == pa。
        (base, base)
    }

    fn dealloc_dma(vaddr : usize, size : usize) {
        let Some(pages) = dma_pages_for(size) else {
            return;
        };
        if vaddr == 0 || vaddr % Self::PAGE_SIZE != 0 {
            return;
        }
        let base_ppn = vaddr / Self::PAGE_SIZE;
        for i in 0..pages {
            let _ =
                frame_alloctor::frame_dealloc_result(mm_api::addr::PhysPageNum(base_ppn + i));
        }
    }
}

#[cfg(feature = "dma")]
fn release(ppns : &[mm_api::addr::PhysPageNum]) {
    for ppn in ppns {
        let _ = frame_alloctor::frame_dealloc_result(*ppn);
    }
}

/// AHCI/SATA 整盘块设备视图（512B 扇区直通）。
#[cfg(feature = "dma")]
pub struct SataBlock(spin::Mutex<isomorphic_drivers::block::ahci::AHCI<FrameProvider>>);

#[cfg(feature = "dma")]
impl SataBlock {
    /// 在给定 AHCI HBA MMIO 窗口上探测并初始化；失败映射为 [`DriverError::IoError`]。
    pub fn new(base : usize, size : usize) -> DriverResult<Self> {
        let ahci = isomorphic_drivers::block::ahci::AHCI::new(base, size)
            .ok_or(DriverError::IoError)?;
        Ok(Self(spin::Mutex::new(ahci)))
    }
}

#[cfg(feature = "dma")]
impl BlockDevice for SataBlock {
    fn total_blocks(&self) -> Option<u64> {
        // AHCI 容量暂不暴露（vendored AHCI 未保存 identify 容量字段）。
        None
    }

    fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
        let Some(_sectors) = sector_count_for(buf.len()) else {
            return Err(DriverError::InvalidParam);
        };
        for (index, chunk) in buf.chunks_exact_mut(BLOCK_SIZE).enumerate() {
            let lba = start_block.0
                                 .checked_add(index as u64)
                                 .ok_or(DriverError::InvalidParam)?;
            self.0
                .lock()
                .read_block(lba as usize, chunk);
        }
        Ok(())
    }

    fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
        let Some(_sectors) = sector_count_for(buf.len()) else {
            return Err(DriverError::InvalidParam);
        };
        for (index, chunk) in buf.chunks_exact(BLOCK_SIZE).enumerate() {
            let lba = start_block.0
                                 .checked_add(index as u64)
                                 .ok_or(DriverError::InvalidParam)?;
            self.0
                .lock()
                .write_block(lba as usize, chunk);
        }
        Ok(())
    }

    fn flush(&mut self) -> DriverResult<()> {
        // polled PIO 写为同步完成，无需额外 flush。
        Ok(())
    }
}

/// PCI 端口 I/O 占位：2K1000 只用 MemoryMapped 访问，端口方法为 no-op。
struct UnusedPort;

impl pci::PortOps for UnusedPort {
    unsafe fn read8(&self, _port : u16) -> u8 { 0 }
    unsafe fn read16(&self, _port : u16) -> u16 { 0 }
    unsafe fn read32(&self, _port : u16) -> u32 { 0 }
    unsafe fn write8(&self, _port : u16, _val : u8) {}
    unsafe fn write16(&self, _port : u16, _val : u16) {}
    unsafe fn write32(&self, _port : u16, _val : u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_sata_mass_storage() {
        assert!(is_sata_mass_storage_class(0x01, 0x06));
        assert!(!is_sata_mass_storage_class(0x01, 0x01));
        assert!(!is_sata_mass_storage_class(0x02, 0x06));
        assert!(!is_sata_mass_storage_class(0, 0));
    }

    #[test]
    fn sector_count_rejects_partial_blocks() {
        assert_eq!(sector_count_for(0), Some(0));
        assert_eq!(sector_count_for(BLOCK_SIZE), Some(1));
        assert_eq!(sector_count_for(BLOCK_SIZE * 4), Some(4));
        assert_eq!(sector_count_for(BLOCK_SIZE - 1), None);
        assert_eq!(sector_count_for(BLOCK_SIZE + 1), None);
    }

    #[test]
    fn dma_pages_round_up_and_reject_zero() {
        assert_eq!(dma_pages_for(0), None);
        assert_eq!(dma_pages_for(1), Some(1));
        assert_eq!(dma_pages_for(4096), Some(1));
        assert_eq!(dma_pages_for(4097), Some(2));
        assert_eq!(dma_pages_for(usize::MAX - 1), None);
    }
}
