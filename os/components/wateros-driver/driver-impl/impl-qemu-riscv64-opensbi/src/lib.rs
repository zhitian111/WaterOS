#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{DeviceInfo, DeviceType, DriverError, DriverResult, IrqLine, MmioRegion};
use block_api_v0::{BlockDevice, BLOCK_SIZE};
use fdt::Fdt;
use spin::Mutex;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr};

static DTB_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);
static DEVICE_INFOS: Mutex<Vec<DeviceInfo>> = Mutex::new(Vec::new());
static VIRTIO_BLK_MMIO: Mutex<Vec<MmioRegion>> = Mutex::new(Vec::new());
static DMA_CURSOR: AtomicUsize = AtomicUsize::new(0x8100_0000);

pub fn init_when_boot(dtb_pa: usize) {
    DTB_BASE_ADDR.store(dtb_pa, Ordering::Release);
}

fn read_fdt() -> DriverResult<Fdt<'static>> {
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb == 0 {
        return Err(DriverError::NotFound);
    }
    // DTB 数据由 bootloader/OpenSBI 提供，内核态早期直接按静态切片读。
    let fdt = unsafe { Fdt::from_ptr(dtb as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
    Ok(fdt)
}

fn parse_mmio_from_reg(reg: &[u8]) -> Option<MmioRegion> {
    // 先按 qemu virt 常见 64-bit address + 64-bit size 处理。
    if reg.len() < 16 {
        return None;
    }
    let mut base_arr = [0u8; 8];
    let mut size_arr = [0u8; 8];
    base_arr.copy_from_slice(&reg[0..8]);
    size_arr.copy_from_slice(&reg[8..16]);
    let base = u64::from_be_bytes(base_arr) as usize;
    let size = u64::from_be_bytes(size_arr) as usize;
    Some(MmioRegion { base, size })
}

fn parse_irq(node: &fdt::node::FdtNode<'_, '_>) -> Option<IrqLine> {
    let irq = node.property("interrupts")?.value;
    if irq.len() < 4 {
        return None;
    }
    let irq_num = u32::from_be_bytes([irq[0], irq[1], irq[2], irq[3]]);

    let parent = node.property("interrupt-parent").and_then(|p| {
        if p.value.len() < 4 {
            None
        } else {
            Some(u32::from_be_bytes([p.value[0], p.value[1], p.value[2], p.value[3]]))
        }
    });
    Some(IrqLine {
        irq: irq_num,
        parent,
    })
}

fn first_compatible(node: &fdt::node::FdtNode<'_, '_>) -> Option<String> {
    let raw = node.property("compatible")?.value;
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    core::str::from_utf8(&raw[..end]).ok().map(String::from)
}

fn is_virtio_mmio(info: &DeviceInfo) -> bool {
    info.compatible == "virtio,mmio"
}

fn mmio_read32(base: usize, word_offset: usize) -> u32 {
    let ptr = (base as *const u32).wrapping_add(word_offset);
    unsafe { core::ptr::read_volatile(ptr) }
}

fn probe_virtio_device_type(mmio: MmioRegion) -> DeviceType {
    let magic = mmio_read32(mmio.base, 0);
    let device_id = mmio_read32(mmio.base, 2);
    // MMIO magic: "virt"
    if magic != 0x74726976 {
        return DeviceType::Unknown;
    }
    match device_id {
        2 => DeviceType::Block,
        1 => DeviceType::Network,
        _ => DeviceType::Unknown,
    }
}

pub fn scan_device_info() -> DriverResult<usize> {
    let fdt = read_fdt()?;
    let mut devices = DEVICE_INFOS.lock();
    devices.clear();

    for node in fdt.all_nodes() {
        let Some(compatible) = first_compatible(&node) else {
            continue;
        };
        let mmio = node.property("reg").and_then(|p| parse_mmio_from_reg(p.value));
        let mut dtype = DeviceType::Unknown;
        if let Some(region) = mmio {
            if compatible == "virtio,mmio" {
                dtype = probe_virtio_device_type(region);
            }
        }

        devices.push(DeviceInfo {
            node_name: String::from(node.name),
            compatible,
            device_type: dtype,
            mmio,
            irq: parse_irq(&node),
        });
    }
    Ok(devices.len())
}

pub fn device_infos() -> &'static Mutex<Vec<DeviceInfo>> {
    &DEVICE_INFOS
}

struct QemuHal;

unsafe impl Hal for QemuHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let bytes = pages * 4096;
        let paddr = DMA_CURSOR.fetch_add(bytes, Ordering::Relaxed) as PhysAddr;
        (paddr, NonNull::new(paddr as *mut u8).unwrap())
    }

    unsafe fn dma_dealloc(
        _paddr: PhysAddr,
        _vaddr: NonNull<u8>,
        _pages: usize,
    ) -> i32 {
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}

pub struct VirtioBlkDevice {
    inner: VirtIOBlk<QemuHal, MmioTransport<'static>>,
}

impl VirtioBlkDevice {
    fn from_mmio(mmio: MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidDtb)?;
        let transport =
            unsafe { MmioTransport::new(header, mmio.size) }.map_err(|_| DriverError::Unsupported)?;
        let inner = VirtIOBlk::<QemuHal, MmioTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        Ok(Self { inner })
    }
}

impl BlockDevice for VirtioBlkDevice {
    fn read_blocks(&mut self, start_block: usize, buf: &mut [u8]) -> DriverResult<()> {
        self.inner
            .read_blocks(start_block, buf)
            .map_err(|_| DriverError::IoError)
    }

    fn write_blocks(&mut self, start_block: usize, buf: &[u8]) -> DriverResult<()> {
        self.inner
            .write_blocks(start_block, buf)
            .map_err(|_| DriverError::IoError)
    }
}

fn probe_virtio_blk() {
    let infos = DEVICE_INFOS.lock();
    let mut blk = VIRTIO_BLK_MMIO.lock();
    blk.clear();
    for info in infos.iter() {
        if info.device_type == DeviceType::Block && is_virtio_mmio(info) {
            if let Some(mmio) = info.mmio {
                blk.push(mmio);
                log::info!(
                    "[driver] found virtio-blk: node={} base={:#x} size={:#x}",
                    info.node_name,
                    mmio.base,
                    mmio.size
                );
            }
        }
    }
}

pub fn virtio_blk_probe_test() -> DriverResult<()> {
    let blk = VIRTIO_BLK_MMIO.lock();
    let Some(mmio) = blk.first().copied() else {
        return Err(DriverError::NotFound);
    };
    drop(blk);
    let mut dev = VirtioBlkDevice::from_mmio(mmio)?;
    let mut buf = [0u8; BLOCK_SIZE];
    dev.read_blocks(0, &mut buf)?;
    log::info!("[driver] virtio-blk read block0 ok, first16={:02x?}", &buf[..16]);
    Ok(())
}

pub fn init_after_boot() -> DriverResult<()> {
    let count = scan_device_info()?;
    log::info!("[driver] dtb scan done, devices={}", count);
    probe_virtio_blk();
    Ok(())
}

pub fn test() {
    log::trace!("[driver-impl-qemu] test begin");
    match init_after_boot() {
        Ok(()) => {
            let _ = virtio_blk_probe_test();
        }
        Err(e) => {
            log::warn!("[driver-impl-qemu] init_after_boot failed: {:?}", e);
        }
    }
    log::trace!("[driver-impl-qemu] test end");
}
