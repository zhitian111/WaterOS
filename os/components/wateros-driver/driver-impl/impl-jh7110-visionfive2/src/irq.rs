//! JH7110 PLIC dispatch boundary.
//!
//! `register_irq_handler` is intentionally explicit: DTB discovery alone never
//! enables a source. Register access and interrupt routing remain待真机测试.

use alloc::vec::Vec;
use api_v0::{DriverError, DriverResult};
use spin::Mutex;

use crate::{plic::{PlicDescription, PlicMmio}, topology::{self, BoardTopology}};

pub type IrqHandler = fn(u32);
static HANDLERS : Mutex<Vec<(u32, IrqHandler)>> = Mutex::new(Vec::new());

pub(crate) fn prepare_current_hart(board : &BoardTopology) -> DriverResult<Option<(usize, usize)>> {
    #[cfg(target_arch = "riscv64")]
    if let Some(description) = board.plic.clone() {
        let hart = platform::arch::cpu::current_cpu_id().raw();
        let Some(context) = description.context_for_hart(hart) else {
            log::warn!("[driver][visionfive2] no supervisor PLIC context for boot hart {}; SEIE remains disabled", hart);
            return Ok(None);
        };
        let plic = PlicMmio::new(description, context)?;
        // SAFETY: the platform memory profile identity-maps the DTB-declared PLIC
        // window. Actual JH7110 register behavior remains待真机测试.
        unsafe { plic.set_threshold(0)?; }
        return Ok(Some((hart, context)));
    }
    #[cfg(target_arch = "riscv64")]
    return Ok(None);
    #[cfg(not(target_arch = "riscv64"))]
    { let _ = board; Ok(None) }
}

#[cfg(target_arch = "riscv64")]
pub(crate) fn enable_current_hart(hart : usize, context : usize) {
    platform::arch::interrupt::enable_external_interrupt();
    log::info!("[driver][visionfive2] enabled supervisor external interrupts hart={} context={}", hart, context);
}

/// Register and enable one PLIC source on every resolved supervisor context.
/// Existing registrations are rejected to avoid ambiguous handler ownership.
pub fn register_irq_handler(source : u32, handler : IrqHandler) -> DriverResult<()> {
    let description = topology::with_topology(|topology| {
        topology.and_then(|board| board.plic.clone())
    }).ok_or(DriverError::NotFound)?;
    if source == 0 || source > description.sources {
        return Err(DriverError::InvalidParam);
    }
    let mut handlers = HANDLERS.lock();
    if handlers.iter().any(|(registered, _)| *registered == source) {
        return Err(DriverError::InvalidParam);
    }
    #[cfg(target_arch = "riscv64")]
    for (context, route) in description.contexts.iter().enumerate() {
        if route.interrupt != 9 || route.hart_id.is_none() {
            continue;
        }
        let plic = PlicMmio::new(description.clone(), context)?;
        // SAFETY: initialization validated the DTB PLIC mapping. Enabling an
        // individual device source still requires真机 IRQ-route verification.
        unsafe { plic.configure_source(source, 1)?; }
    }
    handlers.push((source, handler));
    Ok(())
}

fn handler_for(source : u32) -> Option<IrqHandler> {
    HANDLERS.lock().iter()
            .find(|(registered, _)| *registered == source)
            .map(|(_, handler)| *handler)
}

pub fn handle_external_interrupt(hart_id : usize) -> DriverResult<bool> {
    let description : PlicDescription = topology::with_topology(|topology| {
        topology.and_then(|board| board.plic.clone())
    }).ok_or(DriverError::NotFound)?;
    let context = description.context_for_hart(hart_id).ok_or(DriverError::NotFound)?;
    let plic = PlicMmio::new(description, context)?;
    // SAFETY: SEIE is enabled only after `initialize` validates this hart's
    // context. Claim/complete semantics remain待真机测试.
    let source = unsafe { plic.claim()? };
    if source == 0 {
        return Ok(false);
    }
    if let Some(handler) = handler_for(source) {
        handler(source);
    } else {
        // Prevent an unexpected firmware-enabled source from creating an
        // interrupt storm before its device driver is registered.
        unsafe { plic.disable_source(source)?; }
        log::warn!("[driver][visionfive2] unregistered PLIC source {}; disabled and completing it", source);
    }
    unsafe { plic.complete(source)?; }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_v0::MmioRegion;
    use core::sync::atomic::{AtomicU32, Ordering};
    use crate::plic::ContextInterrupt;

    static HANDLED : AtomicU32 = AtomicU32::new(0);
    fn record(source : u32) { HANDLED.store(source, Ordering::Release); }

    #[test]
    fn dispatches_claimed_source_and_completes_it() {
        let mut words = alloc::vec![0u32; 0x20_1000 / 4];
        let plic = PlicDescription {
            mmio : MmioRegion { base : words.as_mut_ptr() as usize,
                                size : words.len() * 4 },
            sources : 64,
            contexts : alloc::vec![ContextInterrupt { interrupt_controller : 1,
                                                       interrupt : 9,
                                                       hart_id : Some(3) }],
        };
        topology::store(BoardTopology { console_uart : None,
                                        plic : Some(plic) });
        register_irq_handler(17, record).unwrap();
        words[0x20_0004 / 4] = 17;
        assert_eq!(handle_external_interrupt(3), Ok(true));
        assert_eq!(HANDLED.load(Ordering::Acquire), 17);
        assert_eq!(words[0x20_0004 / 4], 17);
        assert_eq!(handle_external_interrupt(4), Err(DriverError::NotFound));
    }
}
