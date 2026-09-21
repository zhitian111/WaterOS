//! Lock-free transport interrupt acknowledgement and deferred event publication.
//!
//! Registrations are permanent: devices and their mapped registers must outlive the kernel.
//! Hard IRQ never takes a device lock, allocates, or accesses a virtqueue.

use api_v0::{DriverError, DriverResult};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub const NETWORK : u32 = 1;
pub const INPUT : u32 = 2;
pub const UART : u32 = 4;
pub const BLOCK : u32 = 8;
pub const DISPLAY : u32 = 16;
const MMIO : usize = 1;
const PCI : usize = 2;
const UART16550 : usize = 3;
const RESERVED : usize = usize::MAX;

struct Route {
    kind : AtomicUsize,
    address : AtomicUsize,
    irq : AtomicU32,
    event : AtomicU32,
}
impl Route {
    const fn new() -> Self {
        Self { kind : AtomicUsize::new(0),
               address : AtomicUsize::new(0),
               irq : AtomicU32::new(0),
               event : AtomicU32::new(0) }
    }
}
static ROUTES : [Route; 64] = [const { Route::new() }; 64];
static EVENTS : AtomicU32 = AtomicU32::new(0);
static NETWORK_ROUTES : AtomicUsize = AtomicUsize::new(0);
static WAIT_HOOK : AtomicUsize = AtomicUsize::new(0);

/// The hook must wait until `ready` is true, without scheduling under driver locks.
/// It must use polling when the current CPU cannot receive the completion IRQ.
pub fn install_wait_hook(hook : fn(&mut dyn FnMut() -> bool)) {
    WAIT_HOOK.store(hook as usize, Ordering::Release);
}

pub fn wait_until(ready : &mut dyn FnMut() -> bool) {
    let hook = WAIT_HOOK.load(Ordering::Acquire);
    if hook != 0 {
        // Only install_wait_hook writes this value, and function pointers remain valid forever.
        let hook : fn(&mut dyn FnMut() -> bool) = unsafe { core::mem::transmute(hook) };
        hook(ready);
    } else {
        while !ready() {
            core::hint::spin_loop();
        }
    }
}

fn register(irq : u32, address : usize, kind : usize, event : u32) -> DriverResult<()> {
    if address == 0 {
        return Err(DriverError::InvalidParam);
    }
    for route in &ROUTES {
        if route.kind
                .compare_exchange(0,
                                  RESERVED,
                                  Ordering::Acquire,
                                  Ordering::Relaxed)
                .is_ok()
        {
            route.address
                 .store(address, Ordering::Relaxed);
            route.irq
                 .store(irq, Ordering::Relaxed);
            route.event
                 .store(event, Ordering::Relaxed);
            route.kind
                 .store(kind, Ordering::Release);
            if event & NETWORK != 0 {
                NETWORK_ROUTES.fetch_add(1, Ordering::Release);
            }
            return Ok(());
        }
    }
    Err(DriverError::Unsupported)
}

/// # Safety
/// `base` must be a live VirtIO MMIO register window of at least 0x68 bytes.
pub unsafe fn register_mmio(irq : u32, base : usize, event : u32) -> DriverResult<()> {
    register(irq, base, MMIO, event)
}

/// # Safety
/// `isr` must be the mapped ISR capability byte of a live VirtIO PCI device.
pub unsafe fn register_pci(irq : u32, isr : usize, event : u32) -> DriverResult<()> {
    register(irq, isr, PCI, event)
}

/// # Safety
/// `base` must be a live byte-register 16550 UART, with DLAB clear.
pub unsafe fn register_uart(irq : u32, base : usize) -> DriverResult<()> {
    register(irq, base, UART16550, UART)?;
    unsafe { core::ptr::write_volatile((base + 1) as *mut u8, 1) };
    Ok(())
}

/// Acknowledge every device on a potentially shared line, then publish event bits.
pub fn handle_irq(irq : u32) -> u32 {
    let mut events = 0;
    for route in &ROUTES {
        let kind = route.kind
                        .load(Ordering::Acquire);
        if kind == 0 ||
           kind == RESERVED ||
           route.irq
                .load(Ordering::Relaxed) !=
           irq
        {
            continue;
        }
        let address = route.address
                           .load(Ordering::Relaxed);
        // Registers were validated by the platform probe and remain mapped permanently.
        let pending = unsafe {
            match kind {
                MMIO => {
                    let status = core::ptr::read_volatile((address + 0x60) as *const u32) & 3;
                    if status != 0 {
                        core::ptr::write_volatile((address + 0x64) as *mut u32, status);
                    }
                    status != 0
                }
                PCI => core::ptr::read_volatile(address as *const u8) & 3 != 0,
                UART16550 => {
                    let pending = core::ptr::read_volatile((address + 2) as *const u8) & 1 == 0;
                    // Leave RX data in FIFO; the reader drains it and rearms RX interrupts.
                    if pending {
                        core::ptr::write_volatile((address + 1) as *mut u8, 0);
                    }
                    pending
                }
                _ => false,
            }
        };
        if pending {
            events |= route.event
                           .load(Ordering::Relaxed);
        }
    }
    EVENTS.fetch_or(events, Ordering::Release);
    events
}

pub fn take_events() -> u32 { EVENTS.swap(0, Ordering::AcqRel) }
pub fn network_irq_ready() -> bool { NETWORK_ROUTES.load(Ordering::Acquire) != 0 }

/// Iterate registered controller sources during boot, before global interrupts are enabled.
pub fn for_each_irq(mut f : impl FnMut(u32)) {
    for route in &ROUTES {
        let kind = route.kind
                        .load(Ordering::Acquire);
        if kind != 0 && kind != RESERVED {
            f(route.irq
                   .load(Ordering::Relaxed));
        }
    }
}

/// Rearm a UART previously masked by its hard IRQ handler. Non-IRQ UARTs stay untouched.
pub fn rearm_uart(base : usize) {
    for route in &ROUTES {
        if route.kind
                .load(Ordering::Acquire) ==
           UART16550 &&
           route.address
                .load(Ordering::Relaxed) ==
           base
        {
            unsafe { core::ptr::write_volatile((base + 1) as *mut u8, 1) };
            return;
        }
    }
}

/// Whether a source belongs to any successfully initialized device.
pub fn is_registered(irq : u32) -> bool {
    ROUTES.iter()
          .any(|r| {
              let k = r.kind
                       .load(Ordering::Acquire);
              k != 0 &&
              k != RESERVED &&
              r.irq
               .load(Ordering::Relaxed) ==
              irq
          })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;

    #[test]
    fn shared_line_acknowledges_only_pending_devices() {
        let first = Box::leak(Box::new([0u32; 32]));
        let second = Box::leak(Box::new([0u32; 32]));
        first[0x60 / 4] = 1;
        unsafe {
            register_mmio(511,
                          first.as_mut_ptr() as usize,
                          NETWORK).unwrap();
            register_mmio(511, second.as_mut_ptr() as usize, INPUT).unwrap();
        }
        assert!(is_registered(511));
        assert!(!is_registered(510));
        assert_eq!(handle_irq(511), NETWORK);
        assert_eq!(first[0x64 / 4], 1);
        assert_eq!(second[0x64 / 4], 0);
        assert_eq!(take_events(), NETWORK);
        first[0x60 / 4] = 0;
        second[0x60 / 4] = 2;
        assert_eq!(handle_irq(511), INPUT);
        assert_eq!(second[0x64 / 4], 2);
        assert_eq!(handle_irq(510), 0);
    }
}

/// Whether a UART RX source was successfully registered during device discovery.
pub fn uart_irq_ready() -> bool {
    ROUTES.iter().any(|route| route.kind.load(Ordering::Acquire) == UART16550)
}
