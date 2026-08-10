//! VirtIO-MMIO 键盘/鼠标/平板实现。

#![no_std]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::{ptr, ptr::NonNull};
use api_v0::{
    AbsoluteAxis, DriverError, DriverResult, InputDevice, InputDeviceId, InputDeviceInfo, InputDeviceKind,
    RawInputEvent,
};
use driver_api::MmioRegion;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use mm_api::addr::PhysPageNum;
use virtio_drivers::device::input::VirtIOInput;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

struct VirtioInputMmioHal;

unsafe impl Hal for VirtioInputMmioHal {
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
        NonNull::new(paddr as *mut u8).expect("virtio-input MMIO address is null")
    }

    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        buffer.as_ptr() as *mut u8 as usize as PhysAddr
    }

    unsafe fn unshare(_paddr : PhysAddr, _buffer : NonNull<[u8]>, _direction : BufferDirection) {}
}

pub struct VirtioInputMmioDevice {
    inner : VirtIOInput<VirtioInputMmioHal, MmioTransport<'static>>,
    info : InputDeviceInfo,
}

impl VirtioInputMmioDevice {
    pub fn from_mmio(mmio : MmioRegion) -> DriverResult<Self> {
        let header = NonNull::new(mmio.base as *mut VirtIOHeader).ok_or(DriverError::InvalidParam)?;
        let transport = unsafe { MmioTransport::new(header, mmio.size) }
            .map_err(|_| DriverError::Unsupported)?;
        let mut inner = VirtIOInput::<VirtioInputMmioHal, MmioTransport>::new(transport)
            .map_err(|_| DriverError::Unsupported)?;
        let info = query_info(&mut inner);
        Ok(Self { inner, info })
    }
}

impl InputDevice for VirtioInputMmioDevice {
    fn info(&self) -> &InputDeviceInfo { &self.info }

    fn pop_event(&mut self) -> DriverResult<Option<RawInputEvent>> {
        Ok(self.inner.pop_pending_event().map(|event| RawInputEvent {
            event_type : event.event_type,
            code : event.code,
            value : event.value as i32,
        }))
    }
}

fn query_info<H : Hal, T : virtio_drivers::transport::Transport>(inner : &mut VirtIOInput<H, T>)
                                                                  -> InputDeviceInfo {
    let name = inner.name().map(|name| String::from(name.trim_end_matches('\0')))
                           .unwrap_or_else(|_| String::from("VirtIO input"));
    let lower = name.to_ascii_lowercase();
    let has_relative = inner.ev_bits(2).is_ok_and(|bits| bits.iter().any(|byte| *byte != 0));
    let has_absolute = inner.ev_bits(3).is_ok_and(|bits| bits.iter().any(|byte| *byte != 0));
    let mut key_bits = [0u8; 64];
    if let Ok(bits) = inner.ev_bits(1) { let len = bits.len().min(key_bits.len()); key_bits[..len].copy_from_slice(&bits[..len]); }
    let mut relative_bits = [0u8; 8];
    if let Ok(bits) = inner.ev_bits(2) { let len = bits.len().min(relative_bits.len()); relative_bits[..len].copy_from_slice(&bits[..len]); }
    let mut absolute_bits = [0u8; 8];
    if let Ok(bits) = inner.ev_bits(3) { let len = bits.len().min(absolute_bits.len()); absolute_bits[..len].copy_from_slice(&bits[..len]); }
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
    let event_types = 1 | ((key_bits.iter().any(|byte| *byte != 0) as u32) << 1)
        | ((has_relative as u32) << 2) | ((has_absolute as u32) << 3);
    InputDeviceInfo { name, kind, absolute_x, absolute_y,
                      id : InputDeviceId { bustype : 0x06, vendor : 0x1af4, product : 0, version : 0 },
                      event_types, key_bits, relative_bits, absolute_bits }
}
