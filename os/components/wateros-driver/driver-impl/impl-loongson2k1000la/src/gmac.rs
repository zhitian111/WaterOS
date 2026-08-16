//! Loongson 2K1000 GMAC0 polling network device.
//!
//! The register model, descriptor layout and initialization sequence follow
//! TGOSKits' `loongson_gmac.rs`. WaterOS currently exposes a simpler polling
//! [`network::NetworkDevice`] trait, so this module does not import TGOSKits'
//! `rd_net` queue/IRQ layer.

use alloc::{boxed::Box, sync::Arc};
use core::{mem::size_of, ptr::addr_of_mut};

use api_v0::{DriverError, DriverResult, MmioRegion};
use common::dtb::{compatible_list, first_mmio_region, read_fdt};
use network::{register_network_device, NetworkDevice};
use spin::Mutex;

const DEVICE_NAME : &str = "ls2k1000-gmac0";
const DEFAULT_MAC_ADDRESS : [u8; 6] = [0x62, 0x19, 0x1A, 0x02, 0xA8, 0x91];
const GMAC0_PADDR : usize = 0x4004_0000;

const MAC_BASE_OFFSET : usize = 0x0000;
const DMA_BASE_OFFSET : usize = 0x1000;
const DEFAULT_MMIO_SIZE : usize = 0x8000;

const GMAC_CONFIG : usize = 0x0000;
const GMAC_FRAME_FILTER : usize = 0x0004;
const GMAC_GMII_ADDR : usize = 0x0010;
const GMAC_GMII_DATA : usize = 0x0014;
const GMAC_FLOW_CONTROL : usize = 0x0018;
const GMAC_VERSION : usize = 0x0020;
const GMAC_INTERRUPT_STATUS : usize = 0x0038;
const GMAC_INTERRUPT_MASK : usize = 0x003C;
const GMAC_ADDR0_HIGH : usize = 0x0040;
const GMAC_ADDR0_LOW : usize = 0x0044;
const GMAC_RGSMII_STATUS : usize = 0x00D8;
const GMAC_MMC_INTR_MASK_RX : usize = 0x010C;
const GMAC_MMC_INTR_MASK_TX : usize = 0x0110;
const GMAC_MMC_RX_IPC_INTR_MASK : usize = 0x0200;

const DMA_BUS_MODE : usize = 0x0000;
const DMA_TX_POLL_DEMAND : usize = 0x0004;
const DMA_RX_POLL_DEMAND : usize = 0x0008;
const DMA_RX_BASE_ADDR : usize = 0x000C;
const DMA_TX_BASE_ADDR : usize = 0x0010;
const DMA_STATUS : usize = 0x0014;
const DMA_CONTROL : usize = 0x0018;
const DMA_INTERRUPT : usize = 0x001C;
const DMA_AXI_BUS_MODE : usize = 0x0028;

const GMII_BUSY : u32 = 1 << 0;
const GMII_CSR_CLK4 : u32 = 1 << 4;
const GMII_REG_SHIFT : u32 = 6;
const GMII_REG_MASK : u32 = 0x1F << GMII_REG_SHIFT;
const GMII_DEV_SHIFT : u32 = 11;
const GMII_DEV_MASK : u32 = 0x1F << GMII_DEV_SHIFT;
const PHY_ADDR : u32 = 0;
const PHY_ID1 : u32 = 2;
const PHY_ID2 : u32 = 3;

const MAC_RX : u32 = 0x0000_0004;
const MAC_TX : u32 = 0x0000_0008;
const MAC_DEFERRAL_CHECK : u32 = 0x0000_0010;
const MAC_BACKOFF_LIMIT : u32 = 0x0000_0060;
const MAC_PAD_CRC_STRIP : u32 = 0x0000_0080;
const MAC_RETRY : u32 = 0x0000_0200;
const MAC_DUPLEX : u32 = 0x0000_0800;
const MAC_LOOPBACK : u32 = 0x0000_1000;
const MAC_RX_OWN : u32 = 0x0000_2000;
const MAC_SPEED_100 : u32 = 0x0000_4000;
const MAC_PORT_SELECT : u32 = 0x0000_8000;
const MAC_JUMBO_FRAME : u32 = 0x0010_0000;
const MAC_FRAME_BURST : u32 = 0x0020_0000;
const MAC_JABBER : u32 = 0x0040_0000;
const MAC_WATCHDOG : u32 = 0x0080_0000;
const MAC_TX_CONFIG : u32 = 0x0100_0000;

const MAC_PROMISCUOUS_MODE : u32 = 0x0000_0001;
const MAC_UCAST_HASH_FILTER : u32 = 0x0000_0002;
const MAC_MCAST_HASH_FILTER : u32 = 0x0000_0004;
const MAC_DEST_ADDR_FILTER : u32 = 0x0000_0008;
const MAC_MULTICAST_FILTER : u32 = 0x0000_0010;
const MAC_BROADCAST : u32 = 0x0000_0020;
const MAC_PASS_CONTROL : u32 = 0x0000_00C0;
const MAC_SRC_ADDR_FILTER : u32 = 0x0000_0200;
const MAC_FILTER : u32 = 0x8000_0000;

const MAC_TX_FLOW_CONTROL : u32 = 0x0000_0002;
const MAC_RX_FLOW_CONTROL : u32 = 0x0000_0004;
const MAC_PAUSE_TIME_MASK : u32 = 0xFFFF_0000;

const MAC_LINK_MODE : u32 = 0x0000_0001;
const MAC_LINK_SPEED_25 : u32 = 0x0000_0002;
const MAC_LINK_SPEED_125 : u32 = 0x0000_0004;
const MAC_LINK_SPEED_MASK : u32 = 0x0000_0006;
const MAC_LINK_STATUS : u32 = 0x0000_0008;

const DMA_RESET_ON : u32 = 0x0000_0001;
const DMA_BURST_LENGTH32 : u32 = 0x0000_2000;
const DMA_BURST_LENGTHX8 : u32 = 0x0100_0000;
const DMA_MIXED_BURST_ENABLE : u32 = 0x0400_0000;
const DMA_RX_START : u32 = 0x0000_0002;
const DMA_TX_SECOND_FRAME : u32 = 0x0000_0004;
const DMA_EN_HW_FLOW_CTRL : u32 = 0x0000_0100;
const DMA_RX_FLOW_CTRL_ACT : u32 = 0x0080_0600;
const DMA_RX_FLOW_CTRL_DEACT : u32 = 0x0040_1800;
const DMA_TX_START : u32 = 0x0000_2000;
const DMA_STORE_AND_FORWARD : u32 = 0x0220_0000;

const DMA_INT_DISABLE : u32 = 0;
const DESC_SIZE1_MASK : u32 = 0x0000_1FFF;
const RX_DESC_END_OF_RING : u32 = 0x0000_8000;
const TX_DESC_END_OF_RING : u32 = 0x0020_0000;
const DESC_TX_FIRST : u32 = 0x1000_0000;
const DESC_TX_LAST : u32 = 0x2000_0000;
const DESC_TX_INT_ENABLE : u32 = 0x4000_0000;
const DESC_RX_LAST : u32 = 0x0000_0100;
const DESC_RX_FIRST : u32 = 0x0000_0200;
const DESC_ERROR : u32 = 0x0000_8000;
const DESC_FRAME_LENGTH_MASK : u32 = 0x3FFF_0000;
const DESC_FRAME_LENGTH_SHIFT : u32 = 16;
const DESC_OWN_BY_DMA : u32 = 0x8000_0000;

const HW_DMA_MASK_32 : u64 = u32::MAX as u64;
const MDIO_TIMEOUT : usize = 100_000;
const DMA_RESET_TIMEOUT : usize = 1_000_000;
const RING_SIZE : usize = 128;
const BUFFER_SIZE : usize = 2048;
const BUFFER_ALIGN : usize = 64;

#[cfg(target_arch = "loongarch64")]
const LOONGARCH64_UNCACHED_WINDOW_BASE : usize = 0x8000_0000_0000_0000;
#[cfg(target_arch = "loongarch64")]
const LOONGARCH64_PHYS_ADDR_MASK : usize = 0x0000_FFFF_FFFF_FFFF;

#[repr(C)]
#[derive(Clone, Copy)]
struct DmaDesc {
    status : u32,
    length : u32,
    buffer1 : u32,
    buffer2 : u32,
}

impl DmaDesc {
    const fn empty() -> Self {
        Self { status : 0,
               length : 0,
               buffer1 : 0,
               buffer2 : 0 }
    }

    fn owned_by_dma(self) -> bool { self.status & DESC_OWN_BY_DMA != 0 }

    fn rx_valid(self) -> bool {
        self.status & DESC_ERROR == 0 &&
        self.status & DESC_RX_FIRST != 0 &&
        self.status & DESC_RX_LAST != 0
    }

    fn rx_length(self) -> usize {
        ((self.status & DESC_FRAME_LENGTH_MASK) >> DESC_FRAME_LENGTH_SHIFT) as usize
    }
}

#[repr(C, align(64))]
struct GmacRing {
    tx : [DmaDesc; RING_SIZE],
    rx : [DmaDesc; RING_SIZE],
}

impl GmacRing {
    const fn new() -> Self {
        Self { tx : [DmaDesc::empty(); RING_SIZE],
               rx : [DmaDesc::empty(); RING_SIZE] }
    }
}

#[repr(C, align(64))]
struct GmacBuffers {
    tx : [[u8; BUFFER_SIZE]; RING_SIZE],
    rx : [[u8; BUFFER_SIZE]; RING_SIZE],
}

impl GmacBuffers {
    const fn new() -> Self {
        Self { tx : [[0; BUFFER_SIZE]; RING_SIZE],
               rx : [[0; BUFFER_SIZE]; RING_SIZE] }
    }
}

#[repr(C, align(64))]
struct AlignedGmacRing(GmacRing);

static mut GMAC_RING : AlignedGmacRing = AlignedGmacRing(GmacRing::new());
static mut GMAC_BUFFERS : GmacBuffers = GmacBuffers::new();

#[derive(Clone, Copy)]
struct Mmio {
    base : *mut u8,
}

impl Mmio {
    fn new(base : *mut u8) -> Self { Self { base } }

    fn read(self, offset : usize) -> u32 {
        unsafe {
            self.base
                .add(offset)
                .cast::<u32>()
                .read_volatile()
        }
    }

    fn write(self, offset : usize, value : u32) {
        unsafe {
            self.base
                .add(offset)
                .cast::<u32>()
                .write_volatile(value)
        };
    }

    fn set_bits(self, offset : usize, bits : u32) { self.write(offset, self.read(offset) | bits); }

    fn clear_bits(self, offset : usize, bits : u32) {
        self.write(offset, self.read(offset) & !bits);
    }
}

unsafe impl Send for Mmio {}
unsafe impl Sync for Mmio {}

#[derive(Clone, Copy)]
struct GmacRegs {
    mac : Mmio,
    dma : Mmio,
}

impl GmacRegs {
    fn new(mmio : MmioRegion) -> Self {
        let base = mmio.base as *mut u8;
        Self { mac : Mmio::new(unsafe { base.add(MAC_BASE_OFFSET) }),
               dma : Mmio::new(unsafe { base.add(DMA_BASE_OFFSET) }) }
    }

    fn wait_mdio_idle(self) -> bool {
        for _ in 0..MDIO_TIMEOUT {
            if self.mac
                   .read(GMAC_GMII_ADDR) &
               GMII_BUSY ==
               0
            {
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    fn mdio_read(self, phy : u32, reg : u32) -> Option<u16> {
        if !self.wait_mdio_idle() {
            return None;
        }
        let addr = ((phy << GMII_DEV_SHIFT) & GMII_DEV_MASK) |
                   ((reg << GMII_REG_SHIFT) & GMII_REG_MASK) |
                   GMII_CSR_CLK4 |
                   GMII_BUSY;
        self.mac
            .write(GMAC_GMII_ADDR, addr);
        self.wait_mdio_idle()
            .then(|| {
                (self.mac
                     .read(GMAC_GMII_DATA) &
                 0xFFFF) as u16
            })
    }

    fn phy_id(self) -> Option<u32> {
        let id1 = self.mdio_read(PHY_ADDR, PHY_ID1)? as u32;
        let id2 = self.mdio_read(PHY_ADDR, PHY_ID2)? as u32;
        Some((id1 << 16) | id2)
    }

    fn link_state(self) -> LinkState {
        LinkState::from_rgsmii(self.mac
                                   .read(GMAC_RGSMII_STATUS))
    }

    fn reset_dma(self) -> DriverResult<()> {
        self.dma
            .write(DMA_BUS_MODE, DMA_RESET_ON);
        for _ in 0..DMA_RESET_TIMEOUT {
            if self.dma
                   .read(DMA_BUS_MODE) &
               DMA_RESET_ON ==
               0
            {
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(DriverError::IoError)
    }

    fn set_mac_address(self, mac : [u8; 6]) {
        let high = ((mac[5] as u32) << 8) | mac[4] as u32;
        let low = ((mac[3] as u32) << 24) |
                  ((mac[2] as u32) << 16) |
                  ((mac[1] as u32) << 8) |
                  mac[0] as u32;
        self.mac
            .write(GMAC_ADDR0_HIGH, high);
        self.mac
            .write(GMAC_ADDR0_LOW, low);
    }

    fn init_dma_regs(self, tx_base : u32, rx_base : u32) {
        self.dma
            .write(DMA_BUS_MODE,
                   DMA_MIXED_BURST_ENABLE | DMA_BURST_LENGTHX8 | DMA_BURST_LENGTH32);
        self.dma
            .write(DMA_CONTROL,
                   DMA_STORE_AND_FORWARD | DMA_TX_SECOND_FRAME);
        self.dma
            .write(DMA_AXI_BUS_MODE, 0xFF | (0x77 << 16));
        self.dma
            .write(DMA_TX_BASE_ADDR, tx_base);
        self.dma
            .write(DMA_RX_BASE_ADDR, rx_base);
    }

    fn init_mac_regs(self) {
        self.mac
            .set_bits(GMAC_CONFIG, MAC_TX_CONFIG);
        self.mac
            .clear_bits(GMAC_CONFIG,
                        MAC_WATCHDOG |
                        MAC_JABBER |
                        MAC_FRAME_BURST |
                        MAC_JUMBO_FRAME |
                        MAC_RX_OWN |
                        MAC_LOOPBACK |
                        MAC_RETRY |
                        MAC_PAD_CRC_STRIP |
                        MAC_DEFERRAL_CHECK |
                        MAC_BACKOFF_LIMIT);
        self.mac
            .set_bits(GMAC_CONFIG, MAC_DUPLEX);
        self.mac
            .clear_bits(GMAC_FRAME_FILTER,
                        MAC_SRC_ADDR_FILTER |
                        MAC_BROADCAST |
                        MAC_MULTICAST_FILTER |
                        MAC_DEST_ADDR_FILTER |
                        MAC_MCAST_HASH_FILTER |
                        MAC_UCAST_HASH_FILTER |
                        MAC_PROMISCUOUS_MODE |
                        MAC_PASS_CONTROL);
        self.mac
            .set_bits(GMAC_FRAME_FILTER, MAC_FILTER);
        let mut dma_ctrl = self.dma
                               .read(DMA_CONTROL);
        dma_ctrl &= !(DMA_RX_FLOW_CTRL_ACT | DMA_RX_FLOW_CTRL_DEACT | DMA_EN_HW_FLOW_CTRL);
        self.dma
            .write(DMA_CONTROL, dma_ctrl);
        let mut flow_ctrl = MAC_PAUSE_TIME_MASK;
        flow_ctrl &= !(MAC_RX_FLOW_CONTROL | MAC_TX_FLOW_CONTROL);
        self.mac
            .write(GMAC_FLOW_CONTROL, flow_ctrl);
    }

    fn configure_link(self, link : LinkState) {
        let old_config = self.mac
                             .read(GMAC_CONFIG);
        let mut config = old_config & !(MAC_PORT_SELECT | MAC_SPEED_100 | MAC_DUPLEX);
        if link.full_duplex {
            config |= MAC_DUPLEX;
        }
        match link.speed_mbps {
            1000 => {}
            100 => config |= MAC_PORT_SELECT | MAC_SPEED_100,
            _ => config |= MAC_PORT_SELECT,
        }
        if config != old_config {
            self.mac
                .write(GMAC_CONFIG, config);
        }
    }

    fn disable_irq(self) {
        self.dma
            .write(DMA_INTERRUPT, DMA_INT_DISABLE);
    }

    fn clear_pending_irq(self) {
        self.mac
            .write(GMAC_MMC_INTR_MASK_TX, u32::MAX);
        self.mac
            .write(GMAC_MMC_INTR_MASK_RX, u32::MAX);
        self.mac
            .write(GMAC_MMC_RX_IPC_INTR_MASK, u32::MAX);
        let status = self.dma
                         .read(DMA_STATUS);
        self.dma
            .write(DMA_STATUS, status);
        let _ = self.mac
                    .read(GMAC_INTERRUPT_STATUS);
        let _ = self.mac
                    .read(GMAC_INTERRUPT_MASK);
    }

    fn start_tx_rx(self) {
        self.mac
            .set_bits(GMAC_CONFIG, MAC_RX | MAC_TX);
        self.dma
            .set_bits(DMA_CONTROL, DMA_RX_START | DMA_TX_START);
        dma_barrier();
        self.dma
            .write(DMA_RX_POLL_DEMAND, 0);
    }

    fn stop_tx_rx(self) {
        self.dma
            .clear_bits(DMA_CONTROL, DMA_RX_START | DMA_TX_START);
        self.mac
            .clear_bits(GMAC_CONFIG, MAC_RX | MAC_TX);
        dma_barrier();
    }
}

unsafe impl Send for GmacRegs {}
unsafe impl Sync for GmacRegs {}

#[derive(Clone, Copy)]
struct LinkState {
    raw : u32,
    up : bool,
    speed_mbps : u32,
    full_duplex : bool,
}

impl LinkState {
    fn from_rgsmii(raw : u32) -> Self {
        let speed_mbps = match raw & MAC_LINK_SPEED_MASK {
            MAC_LINK_SPEED_125 => 1000,
            MAC_LINK_SPEED_25 => 100,
            _ => 10,
        };
        Self { raw,
               up : raw & MAC_LINK_STATUS != 0,
               speed_mbps,
               full_duplex : raw & MAC_LINK_MODE != 0 }
    }
}

#[derive(Clone, Copy)]
struct RingPtrs {
    tx : *mut DmaDesc,
    rx : *mut DmaDesc,
}

// SAFETY: these pointers refer to the single statically allocated GMAC DMA
// rings. Runtime access is serialized through the registered network-device
// mutex and the driver never aliases them as independent mutable owners.
unsafe impl Send for RingPtrs {}
unsafe impl Sync for RingPtrs {}

#[derive(Clone, Copy)]
struct BufferPtrs {
    tx : *mut u8,
    rx : *mut u8,
}

// SAFETY: same ownership model as `RingPtrs`; the buffers are per-descriptor
// static DMA storage and are only touched through the owning device state.
unsafe impl Send for BufferPtrs {}
unsafe impl Sync for BufferPtrs {}

pub struct LoongsonGmacDevice {
    regs : GmacRegs,
    rings : RingPtrs,
    buffers : BufferPtrs,
    mac_address : [u8; 6],
    tx_next : usize,
    tx_busy : usize,
    rx_next : usize,
}

impl LoongsonGmacDevice {
    pub fn from_mmio(mmio : MmioRegion, mac_address : [u8; 6]) -> DriverResult<Self> {
        if mmio.base != GMAC0_PADDR || mmio.size < DMA_BASE_OFFSET + 0x100 {
            return Err(DriverError::Unsupported);
        }
        let regs = GmacRegs::new(mmio);
        let version = regs.mac
                          .read(GMAC_VERSION);
        log::info!("[driver][2k1000] {DEVICE_NAME} version={:#x} base={:#x}",
                   version,
                   mmio.base);
        if let Some(phy_id) = regs.phy_id() {
            log::info!("[driver][2k1000] {DEVICE_NAME} PHY id={:#010x}",
                       phy_id);
        } else {
            log::warn!("[driver][2k1000] {DEVICE_NAME} failed to read PHY id");
        }

        let rings = ring_ptrs();
        let buffers = buffer_ptrs();
        let tx_base = dma_addr32(rings.tx
                                      .cast::<u8>())?;
        let rx_base = dma_addr32(rings.rx
                                      .cast::<u8>())?;
        let _ = dma_addr32(buffer_ptr(buffers.tx, RING_SIZE - 1))?;
        let _ = dma_addr32(buffer_ptr(buffers.rx, RING_SIZE - 1))?;

        unsafe {
            rings.tx
                 .write_bytes(0, RING_SIZE);
            rings.rx
                 .write_bytes(0, RING_SIZE);
            buffers.tx
                   .write_bytes(0, RING_SIZE * BUFFER_SIZE);
            buffers.rx
                   .write_bytes(0, RING_SIZE * BUFFER_SIZE);
        }
        init_tx_ring(rings.tx);
        init_rx_ring(rings.rx, buffers.rx)?;
        dma_barrier();

        regs.stop_tx_rx();
        regs.disable_irq();
        regs.reset_dma()?;
        regs.set_mac_address(mac_address);
        regs.init_dma_regs(tx_base, rx_base);
        regs.init_mac_regs();
        let link = regs.link_state();
        log_link_state(link);
        regs.configure_link(link);
        regs.clear_pending_irq();
        regs.start_tx_rx();

        Ok(Self { regs,
                  rings,
                  buffers,
                  mac_address,
                  tx_next : 0,
                  tx_busy : 0,
                  rx_next : 0 })
    }

    fn reclaim_tx(&mut self) {
        loop {
            let desc = unsafe {
                self.rings
                    .tx
                    .add(self.tx_busy)
                    .read_volatile()
            };
            if desc.owned_by_dma() || desc.length == 0 {
                break;
            }
            let ring_end = self.tx_busy == RING_SIZE - 1;
            unsafe {
                self.rings
                    .tx
                    .add(self.tx_busy)
                    .write_volatile(DmaDesc { status : if ring_end {
                                                  TX_DESC_END_OF_RING
                                              } else {
                                                  0
                                              },
                                              length : 0,
                                              buffer1 : 0,
                                              buffer2 : 0 });
            }
            self.tx_busy = ring_next(self.tx_busy);
        }
    }

    fn rearm_rx(&mut self, index : usize) -> DriverResult<()> {
        let ring_end = index == RING_SIZE - 1;
        let rx_buf = buffer_ptr(self.buffers.rx, index);
        let rx_bus_addr = dma_addr32(rx_buf)?;
        unsafe {
            write_desc_cpu_owned(self.rings
                                     .rx
                                     .add(index),
                                 0,
                                 (BUFFER_SIZE as u32 & DESC_SIZE1_MASK) |
                                 if ring_end { RX_DESC_END_OF_RING } else { 0 },
                                 rx_bus_addr);
        }
        dma_barrier();
        unsafe {
            set_desc_status(self.rings
                                .rx
                                .add(index),
                            DESC_OWN_BY_DMA);
        }
        dma_barrier();
        Ok(())
    }
}

impl NetworkDevice for LoongsonGmacDevice {
    fn mac_address(&self) -> [u8; 6] { self.mac_address }

    fn mtu(&self) -> usize { network::DEFAULT_MTU }

    fn is_link_up(&self) -> bool {
        self.regs
            .link_state()
            .up
    }

    fn send(&mut self, buf : &[u8]) -> DriverResult<()> {
        if buf.is_empty() || buf.len() > BUFFER_SIZE || buf.len() > DESC_SIZE1_MASK as usize {
            return Err(DriverError::InvalidParam);
        }
        self.reclaim_tx();
        let idx = self.tx_next;
        let desc = unsafe {
            self.rings
                .tx
                .add(idx)
                .read_volatile()
        };
        if desc.owned_by_dma() || desc.length != 0 {
            return Err(DriverError::IoError);
        }
        let tx_buf = buffer_ptr(self.buffers.tx, idx);
        let tx_bus_addr = dma_addr32(tx_buf)?;
        unsafe {
            tx_buf.copy_from_nonoverlapping(buf.as_ptr(), buf.len());
        }
        dma_barrier();
        let ring_end = idx == RING_SIZE - 1;
        let status = DESC_OWN_BY_DMA |
                     DESC_TX_INT_ENABLE |
                     DESC_TX_LAST |
                     DESC_TX_FIRST |
                     if ring_end { TX_DESC_END_OF_RING } else { 0 };
        unsafe {
            write_desc_cpu_owned(self.rings
                                     .tx
                                     .add(idx),
                                 status & !DESC_OWN_BY_DMA,
                                 buf.len() as u32 & DESC_SIZE1_MASK,
                                 tx_bus_addr);
        }
        dma_barrier();
        unsafe {
            set_desc_status(self.rings
                                .tx
                                .add(idx),
                            status);
        }
        dma_barrier();
        self.tx_next = ring_next(idx);
        self.regs
            .dma
            .write(DMA_TX_POLL_DEMAND, 0);
        Ok(())
    }

    fn receive(&mut self, buf : &mut [u8]) -> DriverResult<usize> {
        let idx = self.rx_next;
        let desc = unsafe {
            self.rings
                .rx
                .add(idx)
                .read_volatile()
        };
        if desc.owned_by_dma() {
            return Ok(0);
        }
        dma_barrier();
        if !desc.rx_valid() {
            log::warn!("[driver][2k1000] {DEVICE_NAME} RX descriptor error idx={} status={:#x}",
                       idx,
                       desc.status);
            self.rearm_rx(idx)?;
            self.rx_next = ring_next(idx);
            return Ok(0);
        }
        let len = desc.rx_length();
        if len > buf.len() {
            self.rearm_rx(idx)?;
            self.rx_next = ring_next(idx);
            return Err(DriverError::InvalidParam);
        }
        unsafe {
            buf.as_mut_ptr()
               .copy_from_nonoverlapping(buffer_ptr(self.buffers.rx, idx), len);
        }
        self.rearm_rx(idx)?;
        self.rx_next = ring_next(idx);
        Ok(len)
    }
}

pub fn register_from_dtb(dtb_pa : usize) -> DriverResult<usize> {
    let fdt = read_fdt(dtb_pa)?;
    for node in fdt.all_nodes() {
        let compatibles = compatible_list(&node);
        if !is_supported_compatible(&compatibles) {
            continue;
        }
        let Some(mmio) = first_mmio_region(node) else {
            continue;
        };
        if mmio.base != GMAC0_PADDR {
            log::warn!("[driver][2k1000] skip unsupported GMAC node {} base={:#x}",
                       node.name,
                       mmio.base);
            continue;
        }
        let mac_address = mac_address_from_node(&node);
        let device = LoongsonGmacDevice::from_mmio(MmioRegion { base : mmio.base,
                                                                size : if mmio.size == 0 {
                                                                    DEFAULT_MMIO_SIZE
                                                                } else {
                                                                    mmio.size
                                                                } },
                                                   mac_address)?;
        let index = register_network_device(Arc::new(Mutex::new(Box::new(device))));
        log::info!("[driver][2k1000] registered {DEVICE_NAME} network device #{} mac={:02x?}",
                   index,
                   mac_address);
        return Ok(index);
    }
    Err(DriverError::NotFound)
}

pub fn test() {
    assert!(is_supported_compatible(&alloc::vec![
        alloc::string::String::from("snps,dwmac-3.70a"),
    ]));
    assert!(valid_unicast_mac([0x02, 0, 0, 0, 0, 1]));
    assert!(!valid_unicast_mac([0, 0, 0, 0, 0, 0]));
    assert!(!valid_unicast_mac([1, 0, 0, 0, 0, 0]));
    let link = LinkState::from_rgsmii(MAC_LINK_STATUS | MAC_LINK_MODE | MAC_LINK_SPEED_125);
    assert!(link.up);
    assert_eq!(link.speed_mbps, 1000);
}

fn is_supported_compatible(compatibles : &[alloc::string::String]) -> bool {
    compatibles.iter()
               .any(|compatible| {
                   matches!(compatible.as_str(),
                            "snps,dwmac-3.70a" | "snps,arc-dwmac-3.70a")
               })
}

fn mac_address_from_node(node : &fdt::node::FdtNode<'_, '_>) -> [u8; 6] {
    for prop_name in ["local-mac-address",
                      "mac-address"]
    {
        let Some(property) = node.property(prop_name) else {
            continue;
        };
        if property.value.len() < 6 {
            continue;
        }
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&property.value[..6]);
        if valid_unicast_mac(mac) {
            return mac;
        }
    }
    DEFAULT_MAC_ADDRESS
}

fn valid_unicast_mac(mac : [u8; 6]) -> bool { mac != [0; 6] && mac[0] & 1 == 0 }

fn ring_ptrs() -> RingPtrs {
    unsafe {
        let ring = addr_of_mut!(GMAC_RING.0);
        RingPtrs { tx : dma_cpu_ptr(addr_of_mut!((*ring).tx).cast::<DmaDesc>()),
                   rx : dma_cpu_ptr(addr_of_mut!((*ring).rx).cast::<DmaDesc>()) }
    }
}

fn buffer_ptrs() -> BufferPtrs {
    unsafe {
        let buffers = addr_of_mut!(GMAC_BUFFERS);
        BufferPtrs { tx : dma_cpu_ptr(addr_of_mut!((*buffers).tx).cast::<u8>()),
                     rx : dma_cpu_ptr(addr_of_mut!((*buffers).rx).cast::<u8>()) }
    }
}

fn buffer_ptr(base : *mut u8, index : usize) -> *mut u8 { unsafe { base.add(index * BUFFER_SIZE) } }

fn init_tx_ring(tx : *mut DmaDesc) {
    for i in 0..RING_SIZE {
        let ring_end = i == RING_SIZE - 1;
        unsafe {
            tx.add(i)
              .write_volatile(DmaDesc { status : if ring_end { TX_DESC_END_OF_RING } else { 0 },
                                        length : 0,
                                        buffer1 : 0,
                                        buffer2 : 0 });
        }
    }
}

fn init_rx_ring(rx : *mut DmaDesc, rx_buffers : *mut u8) -> DriverResult<()> {
    for i in 0..RING_SIZE {
        let ring_end = i == RING_SIZE - 1;
        let buffer = buffer_ptr(rx_buffers, i);
        let bus_addr = dma_addr32(buffer)?;
        unsafe {
            write_desc_cpu_owned(rx.add(i),
                                 DESC_OWN_BY_DMA,
                                 (BUFFER_SIZE as u32 & DESC_SIZE1_MASK) |
                                 if ring_end { RX_DESC_END_OF_RING } else { 0 },
                                 bus_addr);
        }
    }
    Ok(())
}

unsafe fn write_desc_cpu_owned(desc : *mut DmaDesc, status : u32, length : u32, buffer1 : u32) {
    unsafe {
        desc.write_volatile(DmaDesc { status,
                                      length,
                                      buffer1,
                                      buffer2 : 0 });
    }
}

unsafe fn set_desc_status(desc : *mut DmaDesc, status : u32) {
    unsafe {
        addr_of_mut!((*desc).status).write_volatile(status);
    }
}

fn dma_addr32(ptr : *const u8) -> DriverResult<u32> {
    let paddr = dma_paddr(ptr);
    if paddr > HW_DMA_MASK_32 {
        Err(DriverError::Unsupported)
    } else {
        Ok(paddr as u32)
    }
}

fn dma_cpu_ptr<T>(ptr : *mut T) -> *mut T {
    #[cfg(target_arch = "loongarch64")]
    {
        let phys = dma_paddr(ptr.cast::<u8>()) as usize;
        (LOONGARCH64_UNCACHED_WINDOW_BASE | phys) as *mut T
    }
    #[cfg(not(target_arch = "loongarch64"))]
    {
        ptr
    }
}

fn dma_paddr(ptr : *const u8) -> u64 {
    let addr = ptr as usize;
    #[cfg(target_arch = "loongarch64")]
    {
        (addr & LOONGARCH64_PHYS_ADDR_MASK) as u64
    }
    #[cfg(not(target_arch = "loongarch64"))]
    {
        addr as u64
    }
}

fn ring_next(index : usize) -> usize { (index + 1) % RING_SIZE }

fn log_link_state(link : LinkState) {
    if link.up {
        log::info!("[driver][2k1000] {DEVICE_NAME} link up speed={}Mbps duplex={} raw={:#x}",
                   link.speed_mbps,
                   if link.full_duplex { "full" } else { "half" },
                   link.raw);
    } else {
        log::warn!("[driver][2k1000] {DEVICE_NAME} link down raw={:#x}",
                   link.raw);
    }
}

fn dma_barrier() {
    #[cfg(target_arch = "loongarch64")]
    unsafe {
        core::arch::asm!("dbar 0");
    }
    #[cfg(not(target_arch = "loongarch64"))]
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

const _ : () = {
    assert!(size_of::<DmaDesc>() == 16);
    assert!(size_of::<AlignedGmacRing>().is_multiple_of(BUFFER_ALIGN));
    assert!(size_of::<GmacBuffers>().is_multiple_of(BUFFER_ALIGN));
};

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::*;

    #[test]
    fn recognizes_dwmac_compatibles() {
        assert!(is_supported_compatible(&[String::from("snps,dwmac-3.70a")]));
        assert!(is_supported_compatible(&[String::from("snps,arc-dwmac-3.70a")]));
        assert!(!is_supported_compatible(&[String::from("virtio,mmio")]));
    }

    #[test]
    fn decodes_link_state() {
        let gigabit = LinkState::from_rgsmii(MAC_LINK_STATUS | MAC_LINK_MODE | MAC_LINK_SPEED_125);
        assert!(gigabit.up);
        assert!(gigabit.full_duplex);
        assert_eq!(gigabit.speed_mbps, 1000);
        let hundred = LinkState::from_rgsmii(MAC_LINK_STATUS | MAC_LINK_SPEED_25);
        assert_eq!(hundred.speed_mbps, 100);
        let ten = LinkState::from_rgsmii(0);
        assert!(!ten.up);
        assert_eq!(ten.speed_mbps, 10);
    }

    #[test]
    fn validates_unicast_mac_addresses() {
        assert!(valid_unicast_mac([0x02, 0, 0, 0, 0, 1]));
        assert!(!valid_unicast_mac([0, 0, 0, 0, 0, 0]));
        assert!(!valid_unicast_mac([1, 0, 0, 0, 0, 0]));
    }
}
