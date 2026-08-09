//! JH7110 PLIC dispatch and handler lifecycle.
//!
//! DTB discovery never enables a source by itself. A device driver must quiesce
//! its device-side IRQ and DMA before unregistering. Register access, routing,
//! and cache/device ordering remain待真机测试.

use alloc::{sync::Arc, vec::Vec};
use api_v0::{DriverError, DriverResult};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use spin::Mutex;

use crate::{plic::{PlicDescription, PlicMmio}, topology};

pub type IrqHandler = fn(u32);

struct HandlerEntry {
    source : u32,
    handler : IrqHandler,
    present : Arc<AtomicBool>,
    in_flight : Arc<AtomicUsize>,
}

/// Stable registration token. Slots are append-only and never rebound.
#[derive(Clone)]
pub struct IrqLease {
    slot : usize,
    source : u32,
    present : Arc<AtomicBool>,
    in_flight : Arc<AtomicUsize>,
}

impl IrqLease {
    pub fn slot(&self) -> usize { self.slot }
    pub fn source(&self) -> u32 { self.source }
    pub fn is_present(&self) -> bool { self.present.load(Ordering::Acquire) }
}

static HANDLERS : Mutex<Vec<HandlerEntry>> = Mutex::new(Vec::new());

fn plic_description() -> DriverResult<PlicDescription> {
    topology::with_topology(|topology| topology.and_then(|board| board.plic.clone()))
        .ok_or(DriverError::NotFound)
}

/// Set this hart's threshold and return its raw PLIC context index.
///
/// # Safety
/// The DTB-declared PLIC MMIO window must be mapped for volatile access.
#[allow(dead_code)]
unsafe fn initialize_hart_mmio(description : &PlicDescription,
                               hart_id : usize)
                               -> DriverResult<usize> {
    let context = description.context_for_hart(hart_id).ok_or(DriverError::NotFound)?;
    let plic = PlicMmio::new(description.clone(), context)?;
    unsafe { plic.set_threshold(0)?; }
    Ok(context)
}

pub fn initialize_current_hart(hart_id : usize) -> DriverResult<()> {
    let description = plic_description()?;
    #[cfg(target_arch = "riscv64")]
    {
        // SAFETY: the platform profile identity-maps the DTB PLIC window. Actual
        // JH7110 register behavior remains待真机测试.
        let context = unsafe { initialize_hart_mmio(&description, hart_id)? };
        platform::arch::interrupt::enable_external_interrupt();
        log::info!("[driver][visionfive2] enabled supervisor external interrupts hart={} context={}", hart_id, context);
        Ok(())
    }
    #[cfg(not(target_arch = "riscv64"))]
    { let _ = (description, hart_id); Err(DriverError::Unsupported) }
}

/// Apply one source's enable state to every resolved supervisor context.
///
/// # Safety
/// The DTB-declared PLIC MMIO window must be mapped for volatile access.
#[allow(dead_code)]
unsafe fn set_source_enabled_all(description : &PlicDescription,
                                 source : u32,
                                 enabled : bool)
                                 -> DriverResult<()> {
    for (context, route) in description.contexts.iter().enumerate() {
        if route.interrupt != 9 || route.hart_id.is_none() {
            continue;
        }
        let plic = PlicMmio::new(description.clone(), context)?;
        if enabled {
            unsafe { plic.configure_source(source, 1)?; }
        } else {
            unsafe { plic.disable_source(source)?; }
        }
    }
    Ok(())
}

/// Register and enable one PLIC source on all resolved supervisor contexts.
pub fn register_irq_handler(source : u32, handler : IrqHandler) -> DriverResult<IrqLease> {
    let description = plic_description()?;
    if source == 0 || source > description.sources {
        return Err(DriverError::InvalidParam);
    }
    let present = Arc::new(AtomicBool::new(true));
    let in_flight = Arc::new(AtomicUsize::new(0));
    let slot = {
        let mut handlers = HANDLERS.lock();
        if handlers.iter().any(|entry| {
            entry.source == source && entry.present.load(Ordering::Acquire)
        }) {
            return Err(DriverError::InvalidParam);
        }
        let slot = handlers.len();
        handlers.push(HandlerEntry { source,
                                     handler,
                                     present : present.clone(),
                                     in_flight : in_flight.clone() });
        slot
    };
    #[cfg(target_arch = "riscv64")]
    if let Err(error) = unsafe { set_source_enabled_all(&description, source, true) } {
        present.store(false, Ordering::Release);
        return Err(error);
    }
    Ok(IrqLease { slot, source, present, in_flight })
}

/// Disable a source on every supervisor context, then invalidate its lease.
///
/// The owning device driver must first mask device-local IRQ generation, stop
/// DMA, and establish any required cache/order barriers.
pub fn unregister_irq_handler(lease : &IrqLease) -> DriverResult<bool> {
    if !lease.is_present() {
        return Ok(false);
    }
    let description = plic_description()?;
    #[cfg(target_arch = "riscv64")]
    unsafe { set_source_enabled_all(&description, lease.source, false)?; }
    #[cfg(not(target_arch = "riscv64"))]
    let _ = description;
    if !lease.present.swap(false, Ordering::AcqRel) {
        return Ok(false);
    }
    // Do not call unregister from inside this source's handler. Device-side
    // quiesce plus this drain guarantees no copied handler remains on return.
    while lease.in_flight.load(Ordering::Acquire) != 0 {
        core::hint::spin_loop();
    }
    Ok(true)
}

struct HandlerDispatch {
    handler : IrqHandler,
    in_flight : Arc<AtomicUsize>,
}

impl Drop for HandlerDispatch {
    fn drop(&mut self) { self.in_flight.fetch_sub(1, Ordering::AcqRel); }
}

fn handler_for(source : u32) -> Option<HandlerDispatch> {
    let handlers = HANDLERS.lock();
    let entry = handlers.iter().find(|entry| {
        entry.source == source && entry.present.load(Ordering::Acquire)
    })?;
    entry.in_flight.fetch_add(1, Ordering::AcqRel);
    Some(HandlerDispatch { handler : entry.handler,
                           in_flight : entry.in_flight.clone() })
}

pub fn handle_external_interrupt(hart_id : usize) -> DriverResult<bool> {
    let description = plic_description()?;
    let context = description.context_for_hart(hart_id).ok_or(DriverError::NotFound)?;
    let plic = PlicMmio::new(description, context)?;
    // SAFETY: SEIE is enabled only after this hart's context is initialized.
    let source = unsafe { plic.claim()? };
    if source == 0 {
        return Ok(false);
    }
    if let Some(dispatch) = handler_for(source) {
        (dispatch.handler)(source);
    } else {
        unsafe { plic.disable_source(source)?; }
        log::warn!("[driver][visionfive2] unregistered PLIC source {}; disabled and completing it", source);
    }
    unsafe { plic.complete(source)?; }
    Ok(true)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use api_v0::MmioRegion;
    use core::sync::atomic::AtomicU32;
    use crate::{plic::ContextInterrupt, topology::BoardTopology};
    use std::{sync::Mutex as StdMutex, thread};

    static HANDLED : AtomicU32 = AtomicU32::new(0);
    static BLOCKING_STARTED : AtomicBool = AtomicBool::new(false);
    static BLOCKING_RELEASE : AtomicBool = AtomicBool::new(false);
    static TEST_LOCK : StdMutex<()> = StdMutex::new(());
    fn record(source : u32) { HANDLED.store(source, Ordering::Release); }
    fn blocking_record(_source : u32) {
        BLOCKING_STARTED.store(true, Ordering::Release);
        while !BLOCKING_RELEASE.load(Ordering::Acquire) { core::hint::spin_loop(); }
    }

    fn mock_plic(words : &mut [u32]) -> PlicDescription {
        PlicDescription {
            mmio : MmioRegion { base : words.as_mut_ptr() as usize, size : words.len() * 4 },
            sources : 64,
            contexts : alloc::vec![
                ContextInterrupt { interrupt_controller : 1, interrupt : 11, hart_id : Some(0) },
                ContextInterrupt { interrupt_controller : 1, interrupt : 9, hart_id : Some(0) },
                ContextInterrupt { interrupt_controller : 2, interrupt : 11, hart_id : Some(3) },
                ContextInterrupt { interrupt_controller : 2, interrupt : 9, hart_id : Some(3) },
            ],
        }
    }

    #[test]
    fn initializes_and_toggles_all_supervisor_contexts() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut words = alloc::vec![0u32; 0x20_4000 / 4];
        words[0x20_1000 / 4] = u32::MAX;
        words[0x20_3000 / 4] = u32::MAX;
        let description = mock_plic(&mut words);
        unsafe {
            assert_eq!(initialize_hart_mmio(&description, 0), Ok(1));
            assert_eq!(initialize_hart_mmio(&description, 3), Ok(3));
            set_source_enabled_all(&description, 33, true).unwrap();
        }
        assert_eq!(words[0x20_1000 / 4], 0);
        assert_eq!(words[0x20_3000 / 4], 0);
        assert_ne!(words[0x2084 / 4] & (1 << 1), 0);
        assert_ne!(words[0x2184 / 4] & (1 << 1), 0);
        unsafe { set_source_enabled_all(&description, 33, false).unwrap(); }
        assert_eq!(words[0x2084 / 4] & (1 << 1), 0);
        assert_eq!(words[0x2184 / 4] & (1 << 1), 0);
    }

    #[test]
    fn lease_unregisters_without_slot_reuse() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut words = alloc::vec![0u32; 0x20_4000 / 4];
        let description = mock_plic(&mut words);
        topology::store(BoardTopology { console_uart : None,
                                        plic : Some(description),
                                        ..BoardTopology::default() });
        let first = register_irq_handler(17, record).unwrap();
        words[0x20_1004 / 4] = 17;
        assert_eq!(handle_external_interrupt(0), Ok(true));
        assert_eq!(HANDLED.load(Ordering::Acquire), 17);
        assert_eq!(unregister_irq_handler(&first), Ok(true));
        assert!(!first.is_present());
        assert_eq!(unregister_irq_handler(&first), Ok(false));
        let second = register_irq_handler(17, record).unwrap();
        assert!(second.slot() > first.slot());
    }

    #[test]
    fn unregister_waits_for_copied_handler_to_finish() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut words = alloc::vec![0u32; 0x20_4000 / 4];
        topology::store(BoardTopology { console_uart : None,
                                        plic : Some(mock_plic(&mut words)),
                                        ..BoardTopology::default() });
        BLOCKING_STARTED.store(false, Ordering::Release);
        BLOCKING_RELEASE.store(false, Ordering::Release);
        let lease = register_irq_handler(23, blocking_record).unwrap();
        words[0x20_1004 / 4] = 23;
        let dispatch = thread::spawn(|| handle_external_interrupt(0));
        while !BLOCKING_STARTED.load(Ordering::Acquire) { thread::yield_now(); }
        let unregister_lease = lease.clone();
        let unregister = thread::spawn(move || unregister_irq_handler(&unregister_lease));
        while lease.is_present() { thread::yield_now(); }
        assert!(!unregister.is_finished());
        BLOCKING_RELEASE.store(true, Ordering::Release);
        assert_eq!(dispatch.join().unwrap(), Ok(true));
        assert_eq!(unregister.join().unwrap(), Ok(true));
    }
}
