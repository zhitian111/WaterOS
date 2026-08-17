//! LoongArch QEMU VirtIO-PCI 输入设备实现。

#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::{ptr, ptr::NonNull};
use api_v0::{
    AbsoluteAxis, DriverError, DriverResult, InputDevice, InputDeviceInfo, InputDeviceKind,
    RawInputEvent,
};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::input::VirtIOInput;
use virtio_drivers::transport::DeviceType as VirtioDeviceType;
use virtio_drivers::transport::pci::{self, PciTransport};
use virtio_drivers::transport::pci::bus::{
    BarInfo, Cam, Command, ConfigurationAccess, DeviceFunction, MemoryBarType, MmioCam, PciRoot,
};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtioInputPciProbeInfo {
    pub bus : u8,
    pub device : u8,
    pub function : u8,
    pub vendor_id : u16,
    pub device_id : u16,
}

pub struct VirtioInputPciBarAllocator {
    next : u64,
    end : u64,
}

impl VirtioInputPciBarAllocator {
    pub const fn new(next : u64, end : u64) -> Self { Self { next, end } }

    fn allocate(&mut self, size : u64) -> Option<u64> {
        if size == 0 { return None }
        let align = size.checked_next_power_of_two()?.max(16);
        let start = self.next.checked_add(align - 1)? & !(align - 1);
        let end = start.checked_add(size)?;
        if end > self.end { return None }
        self.next = end;
        Some(start)
    }
}

type VirtioInputPciHal = common::virtio_hal::VirtioHal;

#[cfg(any())]
unsafe impl Hal for VirtioInputPciHal {
    fn dma_alloc(pages : usize, _direction : BufferDirection) -> (PhysAddr, NonNull<u8>) {
        if pages == 0 { return (0, NonNull::dangling()) }
        let mut ppns = Vec::new();
        for _ in 0..pages {
            match frame_alloc_result() {
                Ok(ppn) => ppns.push(ppn),
                Err(_) => {
                    for ppn in ppns { let _ = frame_dealloc_result(ppn); }
                    return (0, NonNull::dangling());
                }
            }
        }
        if (1..pages).any(|index| ppns[index - 1].0 != ppns[index].0 + 1) {
            for ppn in ppns { let _ = frame_dealloc_result(ppn); }
            return (0, NonNull::dangling());
        }
        let address = ppns[pages - 1].0 * PAGE_SIZE;
        unsafe { ptr::write_bytes(address as *mut u8, 0, pages * PAGE_SIZE) };
        (address as PhysAddr, NonNull::new(address as *mut u8).unwrap_or(NonNull::dangling()))
    }

    unsafe fn dma_dealloc(paddr : PhysAddr, _vaddr : NonNull<u8>, pages : usize) -> i32 {
        let base = paddr as usize / PAGE_SIZE;
        for offset in 0..pages { let _ = frame_dealloc_result(PhysPageNum(base + offset)); }
        0
    }

    unsafe fn mmio_phys_to_virt(paddr : PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("virtio-input PCI address is null")
    }

    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}

pub struct VirtioInputPciDevice {
    inner : VirtIOInput<VirtioInputPciHal, PciTransport>,
    info : InputDeviceInfo,
}

impl VirtioInputPciDevice {
    fn from_root<C : ConfigurationAccess>(root : &mut PciRoot<C>,
                                          df : DeviceFunction,
                                          allocator : &mut VirtioInputPciBarAllocator)
                                          -> DriverResult<Self> {
        assign_memory_bars(root, df, allocator)?;
        let (_, command) = root.get_status_command(df);
        root.set_command(df, command | Command::MEMORY_SPACE | Command::BUS_MASTER);
        let transport = PciTransport::new::<VirtioInputPciHal, C>(root, df)
            .map_err(|_| DriverError::Unsupported)?;
        let mut inner = VirtIOInput::<VirtioInputPciHal, PciTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        let info = query_info(&mut inner);
        Ok(Self { inner, info })
    }

    /// 扫描并初始化 bus 0 上全部 VirtIO input 设备。
    ///
    /// # Safety
    /// `config_base` 必须指向内核可访问的 PCIe ECAM。
    pub unsafe fn probe_all_from_ecam(config_base : usize,
                                      allocator : &mut VirtioInputPciBarAllocator)
                                      -> DriverResult<Vec<(Self, VirtioInputPciProbeInfo)>> {
        let cam = unsafe { MmioCam::new(config_base as *mut u8, Cam::Ecam) };
        let mut root = PciRoot::new(cam);
        let candidates : Vec<_> = root.enumerate_bus(0)
                                     .filter(|(_, info)| {
                                         pci::virtio_device_type(info) == Some(VirtioDeviceType::Input)
                                     })
                                     .map(|(df, info)| (df, info.vendor_id, info.device_id))
                                     .collect();
        let mut result = Vec::new();
        for (df, vendor_id, device_id) in candidates {
            let device = Self::from_root(&mut root, df, allocator)?;
            result.push((device,
                         VirtioInputPciProbeInfo { bus : df.bus,
                                                   device : df.device,
                                                   function : df.function,
                                                   vendor_id,
                                                   device_id }));
        }
        Ok(result)
    }
}

impl InputDevice for VirtioInputPciDevice {
    fn info(&self) -> &InputDeviceInfo { &self.info }

    fn pop_event(&mut self) -> DriverResult<Option<RawInputEvent>> {
        Ok(self.inner.pop_pending_event().map(|event| RawInputEvent {
            event_type : event.event_type,
            code : event.code,
            value : event.value as i32,
        }))
    }
}

fn assign_memory_bars<C : ConfigurationAccess>(root : &mut PciRoot<C>,
                                                df : DeviceFunction,
                                                allocator : &mut VirtioInputPciBarAllocator)
                                                -> DriverResult<()> {
    let bars = root.bars(df).map_err(|_| DriverError::Unsupported)?;
    let mut index = 0;
    while index < bars.len() {
        let Some(ref bar) = bars[index] else { index += 1; continue };
        let takes_two = bar.takes_two_entries();
        if let BarInfo::Memory { address_type, size, .. } = *bar {
            if address_type == MemoryBarType::Below1MiB { return Err(DriverError::Unsupported) }
            let assigned = allocator.allocate(size).ok_or(DriverError::Unsupported)?;
            match address_type {
                MemoryBarType::Width32 => root.set_bar_32(df,
                                                         index as u8,
                                                         u32::try_from(assigned)
                                                             .map_err(|_| DriverError::Unsupported)?),
                MemoryBarType::Width64 => root.set_bar_64(df, index as u8, assigned),
                MemoryBarType::Below1MiB => unreachable!(),
            }
        }
        index += if takes_two { 2 } else { 1 };
    }
    Ok(())
}

fn query_info<H : Hal, T : virtio_drivers::transport::Transport>(inner : &mut VirtIOInput<H, T>)
                                                                  -> InputDeviceInfo {
    let name = inner.name().map(|name| String::from(name.trim_end_matches('\0')))
                           .unwrap_or_else(|_| String::from("VirtIO input"));
    let lower = name.to_ascii_lowercase();
    let has_relative = inner.ev_bits(2).is_ok_and(|bits| bits.iter().any(|byte| *byte != 0));
    let has_absolute = inner.ev_bits(3).is_ok_and(|bits| bits.iter().any(|byte| *byte != 0));
    let kind = if lower.contains("keyboard") {
        InputDeviceKind::Keyboard
    } else if has_relative || has_absolute || lower.contains("tablet") || lower.contains("mouse") {
        InputDeviceKind::Pointer
    } else {
        InputDeviceKind::Unknown
    };
    let mut axis = |axis| inner.abs_info(axis).ok().map(|info| AbsoluteAxis {
        minimum : info.min as i32,
        maximum : info.max as i32,
    });
    let absolute_x = if has_absolute { axis(0) } else { None };
    let absolute_y = if has_absolute { axis(1) } else { None };
    InputDeviceInfo { name, kind, absolute_x, absolute_y }
}
