//! Topology-compiled board IRQ owner plan.
//!
//! Plans contain only validated scalar resources. They never construct a
//! volatile backend or touch hardware.

use api_v0::MmioRegion;

use crate::{irq_binding::{InterruptBinding, resolve},
            irq_runtime::RuntimeLayout,
            liointc::Route,
            mmc,
            topology::{BoardTopology, InterruptControllerDescription}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    MmcCommand,
    ApbDmaDeferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationPolicy {
    AckOnly,
    Deferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerPlan {
    pub kind : OwnerKind,
    pub binding : InterruptBinding,
    pub route : Route,
    pub hardware_line : u8,
    pub device_mmio : MmioRegion,
    pub policy : ActivationPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardOwnerPlan {
    pub entries : [OwnerPlan; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerPlanError {
    InvalidRuntimeLayout,
    MissingOrDuplicateMmc,
    MissingOrDuplicateApbDma,
    InvalidMmcResources,
    InvalidBinding,
    MissingController,
    MissingRoute,
    AmbiguousRoute,
    InvalidParent,
    DuplicateIrq,
}

fn controller_for_bank<'a>(topology : &'a BoardTopology,
                       layout : &RuntimeLayout,
                       bank : usize)
                       -> Result<&'a InterruptControllerDescription, OwnerPlanError> {
    let base = layout.controllers.get(bank)
                                 .ok_or(OwnerPlanError::MissingController)?
                                 .main_base;
    let mut matching = topology.interrupt_controllers.iter()
                               .filter(|controller| controller.main_mmio.base == base);
    let controller = matching.next().ok_or(OwnerPlanError::MissingController)?;
    if matching.next().is_some() { return Err(OwnerPlanError::MissingController); }
    Ok(controller)
}

fn route_for(topology : &BoardTopology,
             layout : &RuntimeLayout,
             binding : InterruptBinding)
             -> Result<(Route, u8), OwnerPlanError> {
    let controller = controller_for_bank(topology, layout, binding.global_irq().bank())?;
    let source = 1u32 << binding.global_irq().local();
    let mut matching = controller.parent_source_maps.iter()
                                 .enumerate()
                                 .filter(|(_, map)| **map & source != 0);
    let (slot, _) = matching.next().ok_or(OwnerPlanError::MissingRoute)?;
    if matching.next().is_some() { return Err(OwnerPlanError::AmbiguousRoute); }
    let parent = controller.parent_interrupts[slot].as_ref()
                           .ok_or(OwnerPlanError::InvalidParent)?;
    if parent.cell_count != 1 || parent.cells[0] >= 8 {
        return Err(OwnerPlanError::InvalidParent);
    }
    Ok((Route { core_mask : 1, parent_line : slot as u8 }, parent.cells[0] as u8))
}

pub fn compile(topology : &BoardTopology) -> Result<BoardOwnerPlan, OwnerPlanError> {
    let layout = RuntimeLayout::compile(topology)
        .map_err(|_| OwnerPlanError::InvalidRuntimeLayout)?;
    let mmc_description = match topology.mmc_hosts.as_slice() {
        [description] => description,
        _ => return Err(OwnerPlanError::MissingOrDuplicateMmc),
    };
    let dma_description = match topology.dma_controllers.as_slice() {
        [description] => description,
        _ => return Err(OwnerPlanError::MissingOrDuplicateApbDma),
    };
    let mmc_plan = mmc::plan(mmc_description)
        .map_err(|_| OwnerPlanError::InvalidMmcResources)?;
    let mmc_binding = resolve(topology, &mmc_description.interrupt)
        .map_err(|_| OwnerPlanError::InvalidBinding)?;
    let dma_binding = resolve(topology, &dma_description.interrupt)
        .map_err(|_| OwnerPlanError::InvalidBinding)?;
    if mmc_binding.global_irq() == dma_binding.global_irq() {
        return Err(OwnerPlanError::DuplicateIrq);
    }
    let (mmc_route, mmc_hwi) = route_for(topology, &layout, mmc_binding)?;
    let (dma_route, dma_hwi) = route_for(topology, &layout, dma_binding)?;
    let mut entries = [OwnerPlan { kind : OwnerKind::MmcCommand,
                                   binding : mmc_binding,
                                   route : mmc_route,
                                   hardware_line : mmc_hwi,
                                   device_mmio : mmc_plan.controller_mmio,
                                   policy : ActivationPolicy::AckOnly },
                       OwnerPlan { kind : OwnerKind::ApbDmaDeferred,
                                   binding : dma_binding,
                                   route : dma_route,
                                   hardware_line : dma_hwi,
                                   device_mmio : dma_description.mmio,
                                   policy : ActivationPolicy::Deferred }];
    if entries[0].binding.global_irq().raw() > entries[1].binding.global_irq().raw() {
        entries.swap(0, 1);
    }
    Ok(BoardOwnerPlan { entries })
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use crate::topology::{DmaControllerDescription, InterruptControllerDescription,
                          InterruptSpec, MmcDescription,
                          NamedResource, ResourceSpecifier, CardDetect};
    use super::*;

    fn resource() -> NamedResource {
        NamedResource { name : None,
                        specifier : ResourceSpecifier { provider_phandle : 50, args : vec![0] } }
    }

    fn controller(phandle : u32, base : usize) -> InterruptControllerDescription {
        InterruptControllerDescription {
            phandle : Some(phandle),
            main_mmio : MmioRegion { base, size : 0x40 },
            core_isr : vec![MmioRegion { base : base + 0x100, size : 8 }],
            interrupt_cells : 2,
            parent_interrupts : core::array::from_fn(|_| None),
            parent_source_maps : [0; 4],
        }
    }

    fn topology() -> BoardTopology {
        let mut low = controller(10, 0x1000);
        low.parent_interrupts[0] = Some(InterruptSpec { parent_phandle : 99,
                                                        cells : [2, 0, 0, 0],
                                                        cell_count : 1 });
        low.parent_source_maps = [u32::MAX, 0, 0, 0];
        let mut high = controller(11, 0x1040);
        high.parent_interrupts[1] = Some(InterruptSpec { parent_phandle : 99,
                                                         cells : [3, 0, 0, 0],
                                                         cell_count : 1 });
        high.parent_source_maps = [0, u32::MAX, 0, 0];
        BoardTopology {
            uarts : vec![],
            interrupt_controllers : vec![high, low],
            mmc_hosts : vec![MmcDescription {
                controller_mmio : MmioRegion { base : 0x2000, size : 0x68 },
                auxiliary_mmio : Some(MmioRegion { base : 0x3000, size : 8 }),
                interrupt : InterruptSpec { parent_phandle : 10,
                                            cells : [31, 4, 0, 0], cell_count : 2 },
                clocks : vec![resource()], dma : None, bus_width : 4,
                card_detect : CardDetect::NonRemovable,
                vmmc_supply : None, vqmmc_supply : None,
            }],
            dma_controllers : vec![DmaControllerDescription {
                phandle : 20,
                mmio : MmioRegion { base : 0x4000, size : 8 },
                interrupt : InterruptSpec { parent_phandle : 11,
                                            cells : [13, 4, 0, 0], cell_count : 2 },
                clock : resource(), channel_cells : 1,
            }],
        }
    }

    #[test]
    fn compiles_stable_routes_and_activation_policies() {
        let plan = compile(&topology()).unwrap();
        assert_eq!(plan.entries[0].kind, OwnerKind::MmcCommand);
        assert_eq!(plan.entries[0].binding.global_irq().raw(), 31);
        assert_eq!(plan.entries[0].route, Route { core_mask : 1, parent_line : 0 });
        assert_eq!(plan.entries[0].hardware_line, 2);
        assert_eq!(plan.entries[0].policy, ActivationPolicy::AckOnly);
        assert_eq!(plan.entries[1].kind, OwnerKind::ApbDmaDeferred);
        assert_eq!(plan.entries[1].binding.global_irq().raw(), 45);
        assert_eq!(plan.entries[1].route, Route { core_mask : 1, parent_line : 1 });
        assert_eq!(plan.entries[1].hardware_line, 3);
        assert_eq!(plan.entries[1].policy, ActivationPolicy::Deferred);
    }

    #[test]
    fn rejects_missing_devices_and_duplicate_global_irq() {
        let mut board = topology();
        board.mmc_hosts.clear();
        assert_eq!(compile(&board), Err(OwnerPlanError::MissingOrDuplicateMmc));
        let mut board = topology();
        board.dma_controllers[0].interrupt = board.mmc_hosts[0].interrupt.clone();
        assert_eq!(compile(&board), Err(OwnerPlanError::DuplicateIrq));
    }
}
