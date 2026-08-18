//! VisionFive 2 GMAC1 (JH7110 DWMAC 5.20) integration.
//!
//! The register and descriptor implementation lives in the vendored
//! `dwmac-rs` crate. This module supplies the WaterOS DMA HAL, binds the
//! `starfive,jh7110-dwmac` node from the live DTB, and exposes the kernel's
//! polling [`network::NetworkDevice`] contract.

#![allow(clippy::missing_safety_doc)]

use alloc::{boxed::Box, sync::Arc};
use core::{ptr::NonNull, time::Duration};

use api_v0::{DriverError, DriverResult};
use common::dtb::{compatible_list, first_mmio_region, read_be_u32, read_fdt};
use dwmac::{DwmacHal, DwmacNic, MAX_FRAME_SIZE};
use network::{register_network_device, NetworkDevice};
use spin::Mutex;
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

const GMAC_COMPATIBLE : &str = "starfive,jh7110-dwmac";
const GMAC1_BASE : usize = 0x1604_0000;
const GMAC_DEFAULT_MAC : [u8; 6] = [0x02, 0x56, 0x46, 0x32, 0x00, 0x01];

const SYSCRG_BASE : usize = 0x1302_0000;
const SYSCRG_RESET_ASSERT2 : usize = 0x300;
const SYSCRG_RESET_STATUS2 : usize = 0x310;
const CLOCK_ENABLE : u32 = 1 << 31;
const RESET_STMMACETH : u32 = 0x42;
const RESET_AHB : u32 = 0x43;

/// DMA HAL backed by the linker's physically contiguous 16 MiB DMA pool.
pub struct VisionFive2DwmacHal;

impl DwmacHal for VisionFive2DwmacHal {
    fn dma_alloc(size : usize, _align : usize) -> (dwmac::PhysAddr, NonNull<u8>) {
        let Some(pages) = size.checked_add(PAGE_SIZE - 1)
                              .map(|n| n / PAGE_SIZE)
        else {
            return (0, NonNull::dangling());
        };
        if pages == 0 {
            return (0, NonNull::dangling());
        }
        let (pa, ptr) =
            <common::virtio_hal::VirtioHal as Hal>::dma_alloc(pages, BufferDirection::Both);
        if pa == 0 || ptr == NonNull::dangling() {
            (0, NonNull::dangling())
        } else {
            (pa as usize, ptr)
        }
    }

    unsafe fn dma_dealloc(paddr : dwmac::PhysAddr,
                          vaddr : NonNull<u8>,
                          size : usize,
                          _align : usize)
                          -> i32 {
        let Some(pages) = size.checked_add(PAGE_SIZE - 1)
                              .map(|n| n / PAGE_SIZE)
        else {
            return -1;
        };
        unsafe {
            <common::virtio_hal::VirtioHal as Hal>::dma_dealloc(paddr as PhysAddr, vaddr, pages)
        }
    }

    unsafe fn mmio_phys_to_virt(paddr : dwmac::PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).unwrap_or_else(NonNull::dangling)
    }

    unsafe fn mmio_virt_to_phys(vaddr : NonNull<u8>, _size : usize) -> dwmac::PhysAddr {
        vaddr.as_ptr() as usize
    }

    fn wait_until(duration : Duration) -> Result<(), &'static str> {
        // The board driver's early reset/MDIO waits run before sleeping is
        // available. A bounded spin keeps this HAL independent of scheduler
        // state while still providing a real delay on the target CPU.
        let spins = duration.as_micros()
                            .try_into()
                            .unwrap_or(usize::MAX)
                            .saturating_mul(64)
                            .max(1);
        for _ in 0..spins {
            core::hint::spin_loop();
        }
        Ok(())
    }

    fn configure_platform() -> Result<(), &'static str> { configure_jh7110_clock_reset() }

    fn cache_flush_range(_start : NonNull<u8>, _end : NonNull<u8>) {
        // JH7110's boot profile keeps the DMA pool in the identity-mapped
        // coherent region. The compiler/device fence remains required.
        #[cfg(target_arch = "riscv64")]
        unsafe {
            core::arch::asm!("fence iorw, iorw")
        };
    }
}

fn mmio_read(addr : usize) -> u32 { unsafe { core::ptr::read_volatile(addr as *const u32) } }

fn mmio_write(addr : usize, value : u32) {
    unsafe {
        core::ptr::write_volatile(addr as *mut u32, value);
        #[cfg(target_arch = "riscv64")]
        core::arch::asm!("fence iorw, iorw");
    }
}

fn configure_jh7110_clock_reset() -> Result<(), &'static str> {
    // The DTB lists these five SYSCRG clocks for GMAC1. Preserve mux/divider
    // fields configured by U-Boot and only turn the gates on here.
    for id in [0x61usize, 0x62, 0x66, 0x6A, 0x6B] {
        let address = SYSCRG_BASE.checked_add(id.checked_mul(4)
                                                .ok_or("GMAC clock offset overflow")?)
                                 .ok_or("GMAC clock address overflow")?;
        mmio_write(address,
                   mmio_read(address) | CLOCK_ENABLE);
    }

    let reset_mask = (1u32 << (RESET_STMMACETH % 32)) | (1u32 << (RESET_AHB % 32));
    let assert_addr = SYSCRG_BASE + SYSCRG_RESET_ASSERT2;
    let status_addr = SYSCRG_BASE + SYSCRG_RESET_STATUS2;
    mmio_write(assert_addr,
               mmio_read(assert_addr) & !reset_mask);
    for _ in 0..10_000 {
        if mmio_read(status_addr) & reset_mask == reset_mask {
            return Ok(());
        }
        core::hint::spin_loop();
    }
    Err("GMAC reset deassert timeout")
}

fn enabled(node : &fdt::node::FdtNode<'_, '_>) -> bool {
    node.property("status")
        .and_then(|property| core::str::from_utf8(property.value).ok())
        .is_none_or(|status| {
            matches!(status.trim_end_matches('\0'),
                     "okay" | "ok")
        })
}

fn property_u32(node : &fdt::node::FdtNode<'_, '_>, name : &str, offset : usize) -> Option<u32> {
    node.property(name)
        .and_then(|property| read_be_u32(property.value, offset))
}

fn parse_mac(node : &fdt::node::FdtNode<'_, '_>) -> [u8; 6] {
    for name in ["local-mac-address",
                 "mac-address"]
    {
        if let Some(value) = node.property(name)
                                 .map(|property| property.value)
        {
            if value.len() >= 6 {
                let mut mac = [0; 6];
                mac.copy_from_slice(&value[..6]);
                if mac.iter()
                      .any(|byte| *byte != 0) &&
                   mac[0] & 1 == 0
                {
                    return mac;
                }
            }
        }
    }
    GMAC_DEFAULT_MAC
}

fn parse_phy_addr(fdt : &fdt::Fdt<'_>, node : &fdt::node::FdtNode<'_, '_>) -> u8 {
    let Some(phandle) = property_u32(node, "phy-handle", 0) else {
        return 0;
    };
    let Some(phy) = fdt.find_phandle(phandle) else {
        return 0;
    };
    property_u32(&phy, "reg", 0).and_then(|value| u8::try_from(value).ok())
                                .unwrap_or(0)
}

fn configure_syscon(fdt : &fdt::Fdt<'_>, node : &fdt::node::FdtNode<'_, '_>) -> DriverResult<()> {
    let Some(property) = node.property("starfive,syscon") else {
        // Older VF2 DTBs rely on the bootloader's default RGMII selection.
        return Ok(());
    };
    if property.value.len() < 12 {
        return Err(DriverError::InvalidDtb);
    }
    let provider = read_be_u32(property.value, 0).ok_or(DriverError::InvalidDtb)?;
    let offset = read_be_u32(property.value, 4).ok_or(DriverError::InvalidDtb)? as usize;
    let mask = read_be_u32(property.value, 8).ok_or(DriverError::InvalidDtb)?;
    let provider_node = fdt.find_phandle(provider)
                           .ok_or(DriverError::InvalidDtb)?;
    let mmio = first_mmio_region(provider_node).ok_or(DriverError::InvalidDtb)?;
    let address = mmio.base
                      .checked_add(offset)
                      .ok_or(DriverError::InvalidDtb)?;
    let end = offset.checked_add(4)
                    .ok_or(DriverError::InvalidDtb)?;
    if end > mmio.size || mask == 0 {
        return Err(DriverError::InvalidDtb);
    }
    mmio_write(address, mmio_read(address) | mask);
    Ok(())
}

struct VisionFive2Gmac {
    nic : DwmacNic<VisionFive2DwmacHal>,
    mac : [u8; 6],
}

impl NetworkDevice for VisionFive2Gmac {
    fn mac_address(&self) -> [u8; 6] { self.mac }

    fn mtu(&self) -> usize { network::DEFAULT_MTU }

    fn is_link_up(&self) -> bool {
        self.nic
            .link_up
            .load(core::sync::atomic::Ordering::Acquire)
    }

    fn send(&mut self, buf : &[u8]) -> DriverResult<()> {
        if buf.len() > MAX_FRAME_SIZE {
            return Err(DriverError::InvalidParam);
        }
        self.nic
            .transmit(buf)
            .map_err(|error| {
                log::debug!("[driver][visionfive2] GMAC TX failed: {}",
                            error);
                DriverError::IoError
            })
    }

    fn receive(&mut self, buf : &mut [u8]) -> DriverResult<usize> {
        match self.nic.receive() {
            Ok(packet) => {
                if packet.len() > buf.len() {
                    return Err(DriverError::InvalidParam);
                }
                buf[..packet.len()].copy_from_slice(packet);
                Ok(packet.len())
            }
            Err("RX Error::Again") => Ok(0),
            Err("RX Error::InvalidLength") => Ok(0),
            Err(error) => {
                log::debug!("[driver][visionfive2] GMAC RX failed: {}",
                            error);
                Err(DriverError::IoError)
            }
        }
    }
}

/// Probe and register GMAC1 from the live VisionFive 2 DTB.
pub fn register_from_dtb(dtb_pa : usize) -> DriverResult<usize> {
    let fdt = read_fdt(dtb_pa)?;
    for node in fdt.all_nodes() {
        let compatibles = compatible_list(&node);
        if !enabled(&node) ||
           !compatibles.iter()
                       .any(|item| item == GMAC_COMPATIBLE)
        {
            continue;
        }
        let mmio = first_mmio_region(node).ok_or(DriverError::InvalidDtb)?;
        // JH7110 exposes two DWMAC nodes with the same compatible string.
        // VisionFive 2 routes its RJ45 connector through GMAC1; GMAC0 is the
        // separate RMII controller and must not be initialized by this path.
        if mmio.base != GMAC1_BASE {
            continue;
        }
        if mmio.base == 0 || mmio.size < 0x1200 {
            return Err(DriverError::InvalidDtb);
        }
        configure_syscon(&fdt, &node)?;
        let phy_addr = parse_phy_addr(&fdt, &node);
        let mac = parse_mac(&node);
        let base = NonNull::new(mmio.base as *mut u8).ok_or(DriverError::InvalidDtb)?;
        let mut nic = DwmacNic::<VisionFive2DwmacHal>::init0_with_phy(base, mmio.size, phy_addr)
            .map_err(|error| {
                log::warn!("[driver][visionfive2] DWMAC init failed at {:#x}: {}", mmio.base, error);
                DriverError::IoError
            })?;
        nic.set_mac_address(mac);
        let device = VisionFive2Gmac { nic, mac };
        let index = register_network_device(Arc::new(Mutex::new(Box::new(device))));
        log::info!("[driver][visionfive2] registered GMAC1 #{} base={:#x} phy={} \
                    mac={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                   index,
                   mmio.base,
                   phy_addr,
                   mac[0],
                   mac[1],
                   mac[2],
                   mac[3],
                   mac[4],
                   mac[5]);
        return Ok(index);
    }
    Err(DriverError::NotFound)
}

pub fn test() {
    log::info!("[driver][visionfive2] GMAC1 DWMAC 5.20/YT8531 integration ready; activation is \
                DTB-gated");
}
