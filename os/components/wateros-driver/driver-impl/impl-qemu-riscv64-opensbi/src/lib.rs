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

/// 取节点 `reg` 的第一段映射（地址/长度格式由**父节点**的 `#address-cells` / `#size-cells` 决定）。
/// 使用 `fdt` crate 的 `FdtNode::reg()`，与 QEMU virt 上 `soc` 的 2+2 cells 一致。
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
        let mmio = first_mmio_region(node);
        let mut dtype = DeviceType::Unknown;
        if let Some(region) = mmio {
            if is_virtio_mmio_compatible(&node) {
                dtype = probe_virtio_device_type(region);
            }
        }

        if is_virtio_mmio_compatible(&node) {
            match mmio {
                Some(m) => {
                    let magic = mmio_read32(m.base, 0);
                    let device_id = mmio_read32(m.base, 2);
                    logging::info!(
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
                    logging::warn!(
                        "[driver] dtb virtio-mmio: node={} has no MMIO region (check FdtNode::reg / #address-cells)",
                        node.name
                    );
                }
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
        // `device_type` 已由 MMIO header 探测得到；`DeviceInfo.compatible` 仅保留首条字符串，
        // 可能与 `compatible` 列表顺序不一致，故此处不再依赖 `compatible == "virtio,mmio"`。
        if info.device_type == DeviceType::Block {
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
    let registered = block_device_count();
    logging::info!("[driver] block devices registered={}", registered);
    if registered == 0 {
        logging::warn!(
            "[driver] no block device: devfs will stay empty (this is expected until virtio-blk is present). \
             For QEMU virt add e.g. `-drive file=...,if=none,id=d0 -device virtio-blk-device,drive=d0`. \
             If virtio-mmio lines above show magic!=0x74726976 or wrong mmio, check MMU maps MMIO (0x1xxxxxxx)."
        );
    }
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
