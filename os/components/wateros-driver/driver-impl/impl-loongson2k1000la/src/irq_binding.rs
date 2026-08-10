//! Stable LIOINTC topology binding and level-IRQ lifecycle.
//!
//! Global bank numbers are assigned by ascending LIOINTC main-MMIO address,
//! never by DT node discovery order. Volatile activation remains
//! `UNVERIFIED_ON_HARDWARE` until route/trigger and device-ack behavior is
//! checked on a 2K1000LA board.

use api_v0::{DriverError, DriverResult};

use crate::{irq_domain::{AcknowledgedIrq, GlobalIrq, MAX_BANKS},
            liointc::{LioIntc, RegisterIo, Route, Trigger},
            topology::{BoardTopology, InterruptSpec}};

const IRQ_TYPE_EDGE_RISING : u32 = 1;
const IRQ_TYPE_EDGE_FALLING : u32 = 2;
const IRQ_TYPE_LEVEL_HIGH : u32 = 4;
const IRQ_TYPE_LEVEL_LOW : u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingError {
    MissingProvider,
    DuplicateProvider,
    DuplicateMmio,
    InvalidCells,
    InvalidTrigger,
    TooManyBanks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptBinding {
    provider_phandle : u32,
    global_irq : GlobalIrq,
    trigger : Trigger,
}

impl InterruptBinding {
    pub const fn provider_phandle(self) -> u32 { self.provider_phandle }
    pub const fn global_irq(self) -> GlobalIrq { self.global_irq }
    pub const fn trigger(self) -> Trigger { self.trigger }

    /// Program route and trigger while leaving the source masked.
    pub fn configure_masked<I : RegisterIo>(self,
                                            controller : &mut LioIntc<I>,
                                            route : Route)
                                            -> Result<(), LifecycleFailure<Self>> {
        if controller.bank() != self.global_irq.bank() || route.encode().is_err() {
            return Err(LifecycleFailure { error : DriverError::InvalidParam, state : self });
        }
        if let Err(error) = controller.configure_route(self.global_irq.local(), route) {
            return Err(LifecycleFailure { error, state : self });
        }
        if let Err(error) = controller.set_trigger(self.global_irq.local(), self.trigger) {
            return Err(LifecycleFailure { error, state : self });
        }
        Ok(())
    }

    pub fn arm<I : RegisterIo>(self,
                               controller : &mut LioIntc<I>,
                               route : Route)
                               -> Result<ArmedInterrupt, LifecycleFailure<Self>> {
        self.configure_masked(controller, route)?;
        if let Err(error) = controller.enable(self.global_irq.local()) {
            return Err(LifecycleFailure { error, state : self });
        }
        Ok(ArmedInterrupt { binding : self })
    }
}

pub fn resolve(topology : &BoardTopology,
               spec : &InterruptSpec)
               -> Result<InterruptBinding, BindingError> {
    let controllers = &topology.interrupt_controllers;
    if controllers.len() > MAX_BANKS { return Err(BindingError::TooManyBanks); }
    let matching : alloc::vec::Vec<_> = controllers.iter()
                                                   .filter(|controller| {
                                                       controller.phandle ==
                                                       Some(spec.parent_phandle)
                                                   })
                                                   .collect();
    let controller = match matching.as_slice() {
        [] => return Err(BindingError::MissingProvider),
        [controller] => *controller,
        _ => return Err(BindingError::DuplicateProvider),
    };
    if spec.cell_count != 2 || controller.interrupt_cells != 2 {
        return Err(BindingError::InvalidCells);
    }
    for (index, left) in controllers.iter().enumerate() {
        if left.phandle.is_none() { return Err(BindingError::MissingProvider); }
        for right in &controllers[index + 1..] {
            if left.phandle == right.phandle { return Err(BindingError::DuplicateProvider); }
            if left.main_mmio.base == right.main_mmio.base {
                return Err(BindingError::DuplicateMmio);
            }
        }
    }
    let bank = controllers.iter()
                          .filter(|candidate| {
                              candidate.main_mmio.base < controller.main_mmio.base
                          })
                          .count();
    let global_irq = GlobalIrq::from_bank_local(bank, spec.cells[0])
                               .map_err(|_| BindingError::InvalidCells)?;
    let trigger = match spec.cells[1] {
        IRQ_TYPE_EDGE_RISING => Trigger::EdgeRising,
        IRQ_TYPE_EDGE_FALLING => Trigger::EdgeFalling,
        IRQ_TYPE_LEVEL_HIGH => Trigger::LevelHigh,
        IRQ_TYPE_LEVEL_LOW => Trigger::LevelLow,
        _ => return Err(BindingError::InvalidTrigger),
    };
    Ok(InterruptBinding { provider_phandle : spec.parent_phandle,
                          global_irq,
                          trigger })
}

#[derive(Debug)]
pub struct LifecycleFailure<S> {
    pub error : DriverError,
    pub state : S,
}

#[derive(Debug)]
pub struct ArmedInterrupt {
    binding : InterruptBinding,
}

#[derive(Debug)]
pub struct MaskedInterrupt {
    binding : InterruptBinding,
}

#[derive(Debug)]
pub struct DeviceAckedInterrupt {
    binding : InterruptBinding,
}

pub trait DeviceIrqAck {
    fn clear_interrupt(&mut self) -> DriverResult<()>;
}

impl ArmedInterrupt {
    pub fn claim<I : RegisterIo>(self,
                                 controller : &mut LioIntc<I>)
                                 -> Result<(AcknowledgedIrq, MaskedInterrupt),
                                           LifecycleFailure<Self>> {
        if controller.bank() != self.binding.global_irq.bank() {
            return Err(LifecycleFailure { error : DriverError::InvalidParam, state : self });
        }
        match controller.mask_ack_claim(self.binding.global_irq.bank(),
                                        self.binding.global_irq.local()) {
            Ok(acknowledged) => Ok((acknowledged,
                                    MaskedInterrupt { binding : self.binding })),
            Err(error) => Err(LifecycleFailure { error, state : self }),
        }
    }
}

impl MaskedInterrupt {
    pub fn acknowledge_device<A : DeviceIrqAck>(self,
                                                device : &mut A)
                                                -> Result<DeviceAckedInterrupt,
                                                          LifecycleFailure<Self>> {
        match device.clear_interrupt() {
            Ok(()) => Ok(DeviceAckedInterrupt { binding : self.binding }),
            Err(error) => Err(LifecycleFailure { error, state : self }),
        }
    }
}

impl DeviceAckedInterrupt {
    pub fn rearm<I : RegisterIo>(self,
                                 controller : &mut LioIntc<I>)
                                 -> Result<ArmedInterrupt, LifecycleFailure<Self>> {
        if controller.bank() != self.binding.global_irq.bank() {
            return Err(LifecycleFailure { error : DriverError::InvalidParam, state : self });
        }
        match controller.enable(self.binding.global_irq.local()) {
            Ok(()) => Ok(ArmedInterrupt { binding : self.binding }),
            Err(error) => Err(LifecycleFailure { error, state : self }),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use api_v0::MmioRegion;

    use super::*;
    use crate::topology::{InterruptControllerDescription, InterruptSpec};

    fn controller(phandle : u32, base : usize) -> InterruptControllerDescription {
        InterruptControllerDescription {
            phandle : Some(phandle),
            main_mmio : MmioRegion { base, size : 0x40 },
            core_isr : vec![MmioRegion { base : base - 0x400, size : 8 }],
            interrupt_cells : 2,
            parent_interrupts : core::array::from_fn(|_| None),
            parent_source_maps : [u32::MAX, 0, 0, 0],
        }
    }

    fn topology(controllers : alloc::vec::Vec<InterruptControllerDescription>)
                -> BoardTopology {
        BoardTopology { uarts : vec![],
                        interrupt_controllers : controllers,
                        mmc_hosts : vec![],
                        dma_controllers : vec![], networks : vec![] }
    }

    fn spec(provider : u32, local : u32, trigger : u32) -> InterruptSpec {
        InterruptSpec { parent_phandle : provider,
                        cells : [local, trigger, 0, 0],
                        cell_count : 2 }
    }

    #[derive(Default)]
    struct ModelIo {
        reads : alloc::vec::Vec<(usize, u32)>,
        writes32 : alloc::vec::Vec<(usize, u32)>,
        writes8 : alloc::vec::Vec<(usize, u8)>,
    }

    impl RegisterIo for ModelIo {
        fn read32(&self, address : usize) -> u32 {
            self.reads.iter()
                      .rev()
                      .find(|(candidate, _)| *candidate == address)
                      .map(|(_, value)| *value)
                      .unwrap_or(0)
        }
        fn write32(&mut self, address : usize, value : u32) {
            self.writes32.push((address, value));
            self.reads.push((address, value));
        }
        fn write8(&mut self, address : usize, value : u8) {
            self.writes8.push((address, value));
        }
    }

    struct MockDeviceAck {
        fail : bool,
        calls : usize,
    }

    impl DeviceIrqAck for MockDeviceAck {
        fn clear_interrupt(&mut self) -> DriverResult<()> {
            self.calls += 1;
            if self.fail { Err(DriverError::IoError) } else { Ok(()) }
        }
    }

    #[test]
    fn resolver_is_stable_across_controller_discovery_order() {
        let ordered = topology(vec![controller(10, 0x1fe0_1400),
                                    controller(11, 0x1fe0_1440)]);
        let reversed = topology(vec![controller(11, 0x1fe0_1440),
                                     controller(10, 0x1fe0_1400)]);
        let expected = InterruptBinding { provider_phandle : 11,
                                          global_irq : GlobalIrq::from_bank_local(1, 13).unwrap(),
                                          trigger : Trigger::LevelHigh };
        assert_eq!(resolve(&ordered, &spec(11, 13, IRQ_TYPE_LEVEL_HIGH)), Ok(expected));
        assert_eq!(resolve(&reversed, &spec(11, 13, IRQ_TYPE_LEVEL_HIGH)), Ok(expected));
    }

    #[test]
    fn resolver_rejects_ambiguous_or_invalid_specs() {
        let valid = controller(10, 0x1fe0_1400);
        assert_eq!(resolve(&topology(vec![valid.clone()]), &spec(99, 1, 4)),
                   Err(BindingError::MissingProvider));
        assert_eq!(resolve(&topology(vec![valid.clone(), valid.clone()]), &spec(10, 1, 4)),
                   Err(BindingError::DuplicateProvider));
        let duplicate_mmio = controller(11, 0x1fe0_1400);
        assert_eq!(resolve(&topology(vec![valid.clone(), duplicate_mmio]), &spec(10, 1, 4)),
                   Err(BindingError::DuplicateMmio));
        assert_eq!(resolve(&topology(vec![valid.clone()]), &spec(10, 32, 4)),
                   Err(BindingError::InvalidCells));
        assert_eq!(resolve(&topology(vec![valid]), &spec(10, 1, 0)),
                   Err(BindingError::InvalidTrigger));
    }

    #[test]
    fn lifecycle_requires_device_ack_before_reenable() {
        const BASE : usize = 0x1000;
        let binding = resolve(&topology(vec![controller(10, 0x1fe0_1400)]),
                              &spec(10, 13, IRQ_TYPE_LEVEL_HIGH)).unwrap();
        let mut lio = LioIntc::new(ModelIo::default(), 0, BASE, &[0x2000]).unwrap();
        let armed = binding.arm(&mut lio, Route { core_mask : 1, parent_line : 0 }).unwrap();
        let (acknowledged, masked) = armed.claim(&mut lio).unwrap();
        assert_eq!(acknowledged.irq(), binding.global_irq());
        let mut device = MockDeviceAck { fail : true, calls : 0 };
        let failure = match masked.acknowledge_device(&mut device) {
            Err(failure) => failure,
            Ok(_) => panic!("failed device ack advanced lifecycle"),
        };
        assert_eq!(device.calls, 1);
        device.fail = false;
        let device_acked = failure.state.acknowledge_device(&mut device).unwrap();
        device_acked.rearm(&mut lio).unwrap();
        let io = lio.into_inner();
        assert_eq!(device.calls, 2);
        assert_eq!(io.writes32.iter()
                              .filter(|(address, _)| *address == BASE + 0x28)
                              .count(),
                   2);
        assert_eq!(io.writes32.iter()
                              .filter(|(address, _)| *address == BASE + 0x2c)
                              .count(),
                   1);
    }
}
