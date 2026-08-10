//! VirtIO-MMIO 键盘/鼠标/平板实现。

#![no_std]
extern crate alloc;

use alloc::string::String;
use core::ptr::NonNull;
use api_v0::{
    AbsoluteAxis, DriverError, DriverResult, InputDevice, InputDeviceInfo, InputDeviceKind,
    RawInputEvent,
};
use driver_api::MmioRegion;
use driver_common::virtio_dma;
use virtio_drivers::device::input::VirtIOInput;
use virtio_drivers::transport::mmio::{MmioTransport, VirtIOHeader};
use virtio_drivers::{BufferDirection, Hal, PhysAddr, PAGE_SIZE};

struct VirtioInputMmioHal;

unsafe impl Hal for VirtioInputMmioHal {
    fn dma_alloc(pages : usize, _direction : BufferDirection) -> (PhysAddr, NonNull<u8>) {
        virtio_dma::alloc(pages)
            .map(|(address, pointer)| (address as PhysAddr, pointer))
            .unwrap_or((0, NonNull::dangling()))
    }

    unsafe fn dma_dealloc(paddr : PhysAddr, vaddr : NonNull<u8>, pages : usize) -> i32 {
        unsafe { virtio_dma::dealloc(paddr as u64, vaddr, pages) }
    }

    unsafe fn mmio_phys_to_virt(paddr : PhysAddr, _size : usize) -> NonNull<u8> {
        NonNull::new(paddr as *mut u8).expect("virtio-input MMIO address is null")
    }

    unsafe fn share(buffer : NonNull<[u8]>, _direction : BufferDirection) -> PhysAddr {
        unsafe { virtio_dma::share_identity(buffer) as PhysAddr }
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
