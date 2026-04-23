#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{DeviceInfo, DeviceType, DriverError, DriverResult, IrqLine, MmioRegion};
use block_api_v0::{block_device_count, register_block_device, BlockDevice, Lba, BLOCK_SIZE};
use fdt::Fdt;
use fs::devfs::active_impl as devfs_impl;
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

fn read_be_u32(raw: &[u8], offset: usize) -> Option<u32> {
    let bytes = raw.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn parse_cell_value(raw: &[u8], cells: usize) -> Option<usize> {
    if cells == 0 || cells > 2 || raw.len() < cells * 4 {
        return None;
    }

    let mut value: u64 = 0;
    for idx in 0..cells {
        value = (value << 32) | u64::from(read_be_u32(raw, idx * 4)?);
    }
    usize::try_from(value).ok()
}

fn read_cell_count(node: &fdt::node::FdtNode<'_, '_>, key: &str, default: usize) -> usize {
    node.property(key)
        .and_then(|p| read_be_u32(p.value, 0))
        .map(|v| v as usize)
        .unwrap_or(default)
}

fn parse_mmio_from_reg_with_cells(
    node: &fdt::node::FdtNode<'_, '_>,
    reg: &[u8],
) -> Option<MmioRegion> {
    // FDT crate 0.1.5 未暴露父节点查询，这里按节点局部声明读取并回退 qemu virt 默认值。
    let address_cells = read_cell_count(node, "#address-cells", 2);
    let size_cells = read_cell_count(node, "#size-cells", 1);
    let base_len = address_cells.checked_mul(4)?;
    let size_len = size_cells.checked_mul(4)?;
    let total = base_len.checked_add(size_len)?;
    if reg.len() < total {
        return None;
    }

    let base = parse_cell_value(&reg[..base_len], address_cells)?;
    let size = parse_cell_value(&reg[base_len..base_len + size_len], size_cells)?;
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

fn first_compatible(node: &fdt::node::FdtNode<'_, '_>) -> Option<String> {
    compatible_list(node).into_iter().next()
}

fn is_virtio_mmio_compatible(node: &fdt::node::FdtNode<'_, '_>) -> bool {
    compatible_list(node)
        .iter()
        .any(|item| item.as_str() == "virtio,mmio")
}

fn is_virtio_mmio(info: &DeviceInfo) -> bool {
    info.compatible.as_str() == "virtio,mmio"
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
        let mmio = node
            .property("reg")
            .and_then(|p| parse_mmio_from_reg_with_cells(&node, p.value));
        let mut dtype = DeviceType::Unknown;
        if let Some(region) = mmio {
            if is_virtio_mmio_compatible(&node) {
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
    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        self.inner
            .read_blocks(start_block.0 as usize, buf)
            .map_err(|_| DriverError::IoError)
    }

    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        self.inner
            .write_blocks(start_block.0 as usize, buf)
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
                match VirtioBlkDevice::from_mmio(mmio) {
                    Ok(dev) => {
                        let idx = register_block_device(Arc::new(Mutex::new(Box::new(dev))));
                        logging::info!("[driver] registered virtio-blk #{}", idx);
                    }
                    Err(err) => {
                        logging::warn!(
                            "[driver] failed to init virtio-blk at base={:#x}: {:?}",
                            mmio.base,
                            err
                        );
                    }
                }
                logging::info!(
                    "[driver] found virtio-blk: node={} base={:#x} size={:#x}",
                    info.node_name,
                    mmio.base,
                    mmio.size
                );
            }
        }
    }
}

fn sync_devfs() -> usize {
    let node_count = devfs_impl::refresh();
    logging::info!("[driver] devfs refreshed, nodes={}", node_count);
    node_count
}

fn dump_device_and_devfs_info() {
    let infos = DEVICE_INFOS.lock();
    for (idx, info) in infos.iter().enumerate() {
        logging::info!(
            "[driver][test] dev#{} node={} compatible={} type={:?} mmio={:?} irq={:?}",
            idx,
            info.node_name,
            info.compatible,
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
    let count = scan_device_info()?;
    logging::info!("[driver] dtb scan done, devices={}", count);
    probe_virtio_blk();
    logging::info!("[driver] block devices registered={}", block_device_count());
    let _ = sync_devfs();
    Ok(())
}

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
