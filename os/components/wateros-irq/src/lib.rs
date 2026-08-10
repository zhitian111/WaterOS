#![no_std]

//! WaterOS dynamic external-interrupt registration and dispatch.

use core::hint::spin_loop;
use core::sync::atomic::{AtomicUsize, Ordering};

use irq_framework::{
    AutoEnable, CpuId, HwIrq, IrqAffinity, IrqDomainId, IrqId, IrqOps, IrqRequest, Registry,
};
pub use irq_framework::{IrqContext, IrqError, IrqHandle, IrqReturn, ShareMode};
use spin::Once;

const PLATFORM_DOMAIN : IrqDomainId = IrqDomainId(0);
static REGISTRY : Once<Registry<WaterIrqOps>> = Once::new();
static IN_EXTERNAL_IRQ_CPUS : AtomicUsize = AtomicUsize::new(0);

struct WaterIrqOps;

impl IrqOps for WaterIrqOps {
    type LocalIrqState = platform::arch::interrupt::ArchInterruptState;

    fn current_cpu(&self) -> CpuId { CpuId(platform::arch::cpu::current_cpu_id().raw()) }

    fn cpu_online(&self, cpu : CpuId) -> bool {
        task::online_cpu_mask().contains(task::CpuId::from_raw(cpu.0))
    }

    fn in_irq_context(&self) -> bool {
        let cpu = self.current_cpu().0;
        cpu < usize::BITS as usize &&
        IN_EXTERNAL_IRQ_CPUS.load(Ordering::Acquire) & (1usize << cpu) != 0
    }

    fn local_irq_save(&self) -> Self::LocalIrqState {
        let state =
            platform::interrupt::read_global_interrupt_state().expect("read local IRQ state");
        platform::interrupt::disable_global_interrupt().expect("disable local IRQs");
        state
    }

    fn local_irq_restore(&self, state : Self::LocalIrqState) {
        platform::interrupt::restore_global_interrupt_state(state).expect("restore local IRQ \
                                                                           state");
    }

    fn run_on_cpu_sync(&self,
                       cpu : CpuId,
                       f : unsafe fn(*mut ()),
                       arg : *mut ())
                       -> Result<(), IrqError> {
        if cpu != self.current_cpu() {
            return Err(IrqError::Unsupported);
        }
        unsafe { f(arg) };
        Ok(())
    }

    fn set_affinity(&self, _irq : IrqId, affinity : IrqAffinity) -> Result<(), IrqError> {
        match affinity {
            IrqAffinity::Any => Ok(()),
            IrqAffinity::Fixed(cpu) if cpu == self.current_cpu() => Ok(()),
            IrqAffinity::Fixed(_) => Err(IrqError::Unsupported),
        }
    }

    fn set_enabled(&self,
                   irq : IrqId,
                   cpu : Option<CpuId>,
                   enabled : bool)
                   -> Result<(), IrqError> {
        validate_irq(irq)?;
        let cpu = cpu.unwrap_or_else(|| self.current_cpu());
        platform::external_irq::set_enabled(irq.hwirq.0, cpu.0, enabled).map_err(|_| {
                                                                            IrqError::Controller
                                                                        })
    }

    fn is_enabled(&self, irq : IrqId, cpu : Option<CpuId>) -> Result<bool, IrqError> {
        validate_irq(irq)?;
        let cpu = cpu.unwrap_or_else(|| self.current_cpu());
        platform::external_irq::is_enabled(irq.hwirq.0, cpu.0).map_err(|_| IrqError::Controller)
    }

    fn is_pending(&self, irq : IrqId, _cpu : Option<CpuId>) -> Result<bool, IrqError> {
        validate_irq(irq)?;
        platform::external_irq::is_pending(irq.hwirq.0).map_err(|_| IrqError::Controller)
    }

    fn is_in_service(&self, _irq : IrqId, _cpu : Option<CpuId>) -> Result<bool, IrqError> {
        Err(IrqError::Unsupported)
    }

    fn relax(&self) { spin_loop(); }
}

fn validate_irq(irq : IrqId) -> Result<(), IrqError> {
    if irq.domain == PLATFORM_DOMAIN && irq.hwirq.0 != 0 {
        Ok(())
    } else {
        Err(IrqError::InvalidIrq)
    }
}

fn registry() -> &'static Registry<WaterIrqOps> {
    REGISTRY.get()
            .expect("wateros_irq::init must run before IRQ use")
}

/// Initializes the platform interrupt controller and the dynamic registry.
pub fn init() -> Result<(), IrqError> {
    REGISTRY.call_once(|| Registry::new(WaterIrqOps));
    init_current_cpu()?;
    Ok(())
}

/// Initializes the local external-interrupt context for an additional CPU.
pub fn init_current_cpu() -> Result<(), IrqError> {
    platform::external_irq::init_current_cpu().map_err(|_| IrqError::Controller)
}

/// Registers a global IRQ action in the platform controller domain.
pub fn request(hwirq : u32,
               shared : bool,
               handler : impl FnMut(IrqContext) -> IrqReturn + Send + 'static)
               -> Result<IrqHandle, IrqError> {
    let share_mode = if shared {
        ShareMode::Shared
    } else {
        ShareMode::Exclusive
    };
    let request_cpu = platform::arch::cpu::current_cpu_id().raw();
    let handle = registry().request(IrqId::new(PLATFORM_DOMAIN, HwIrq(hwirq)),
                                    IrqRequest::new(handler).share_mode(share_mode)
                                                            .auto_enable(AutoEnable::Yes))?;
    // A serialized VirtQueue needs one delivery target, not one PLIC delivery per hart. Keep the
    // line on the stable BSP context; tasks on other CPUs observe completion through the shared
    // generation/used ring without rewriting PLIC affinity for every request.
    if request_cpu != 0 {
        platform::external_irq::set_enabled(hwirq, request_cpu, false)
            .map_err(|_| IrqError::Controller)?;
    }
    platform::external_irq::set_enabled(hwirq, 0, true).map_err(|_| IrqError::Controller)?;
    Ok(handle)
}

/// Returns whether the current task may sleep waiting for an external IRQ.
///
/// A syscall trap normally has global interrupts masked. That does not prevent waiting: the
/// scheduler switches to an interrupt-enabled runnable or idle task, then restores this task's
/// original interrupt state when it is woken. During SMP bring-up the scheduler exposes a logical
/// idle task while the CPU still physically runs on its boot stack, so idle/boot paths must poll.
pub fn can_wait() -> bool {
    let cpu = task::CpuId::from_raw(platform::arch::cpu::current_cpu_id().raw());
    task::current_task_id().is_some() &&
    task::cpu_snapshot(cpu).is_some_and(|snapshot| {
                                    !snapshot.boot_context_active && !snapshot.current_is_idle
                                }) &&
    !WaterIrqOps.in_irq_context()
}

/// Waits on the current kernel stack for IRQ completion or a transport-level completion fallback.
///
/// This deliberately does not enter the task scheduler or change interrupt state: block I/O may be issued while legacy
/// filesystem code owns spin-based metadata locks. Scheduling another task in that state can make
/// every hart spin on the same lock with interrupts masked.
pub fn wait_for_interrupt(mut completed : impl FnMut() -> bool) {
    while !completed() {
        spin_loop();
    }
}

/// Claims and dispatches all pending external interrupts on the current CPU.
pub fn dispatch_external() {
    let cpu = platform::arch::cpu::current_cpu_id().raw();
    let cpu_bit = (cpu < usize::BITS as usize).then(|| 1usize << cpu)
                                              .unwrap_or(0);
    IN_EXTERNAL_IRQ_CPUS.fetch_or(cpu_bit, Ordering::AcqRel);
    while let Some(hwirq) = platform::external_irq::claim(cpu) {
        let irq = IrqId::new(PLATFORM_DOMAIN, HwIrq(hwirq));
        let outcome = registry().dispatch(irq, CpuId(cpu));
        platform::external_irq::complete(cpu, hwirq);
        if !outcome.handled {
            log::warn!("[irq] unhandled external IRQ {} on cpu {}",
                       hwirq,
                       cpu);
        }
    }
    IN_EXTERNAL_IRQ_CPUS.fetch_and(!cpu_bit, Ordering::AcqRel);
}
