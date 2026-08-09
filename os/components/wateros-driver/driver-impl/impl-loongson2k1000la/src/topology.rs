use alloc::vec::Vec;

use api_v0::{DriverError, DriverResult, MmioRegion};
use common::dtb::read_be_u32;

const MAX_INTERRUPT_CELLS : usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptSpec {
    pub parent_phandle : u32,
    pub cells : [u32; MAX_INTERRUPT_CELLS],
    pub cell_count : u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UartDescription {
    pub mmio : MmioRegion,
    pub interrupt : InterruptSpec,
    pub clock_hz : u32,
    pub register_shift : u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptControllerDescription {
    pub phandle : u32,
    pub mmio : MmioRegion,
    pub interrupt_cells : u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcDescription {
    pub controller_mmio : MmioRegion,
    pub auxiliary_mmio : Option<MmioRegion>,
    pub interrupt : InterruptSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTopology {
    pub uarts : Vec<UartDescription>,
    pub interrupt_controllers : Vec<InterruptControllerDescription>,
    pub mmc_hosts : Vec<MmcDescription>,
}

fn property_u32(node : fdt::node::FdtNode<'_, '_>, name : &str) -> Option<u32> {
    let value = node.property(name)?
                    .value;
    (value.len() == 4).then(|| read_be_u32(value, 0))
                      .flatten()
}

fn has_compatible(node : fdt::node::FdtNode<'_, '_>, expected : &str) -> bool {
    node.property("compatible")
        .map(|property| {
            property.value
                    .split(|byte| *byte == 0)
                    .any(|item| item == expected.as_bytes())
        })
        .unwrap_or(false)
}

fn enabled(node : fdt::node::FdtNode<'_, '_>) -> DriverResult<bool> {
    let Some(raw) = node.property("status")
                        .map(|property| property.value)
    else {
        return Ok(true);
    };
    let Some(raw) = raw.strip_suffix(&[0]) else {
        return Err(DriverError::InvalidDtb);
    };
    match raw {
        b"okay" | b"ok" => Ok(true),
        b"disabled" | b"reserved" | b"fail" | b"failed" => Ok(false),
        _ => Err(DriverError::InvalidDtb),
    }
}

fn regions(node : fdt::node::FdtNode<'_, '_>) -> DriverResult<Vec<MmioRegion>> {
    let raw = node.property("reg")
                  .ok_or(DriverError::InvalidDtb)?
                  .value;
    let encoded_len = node.raw_reg()
                          .ok_or(DriverError::InvalidDtb)?
                          .try_fold(0usize, |total, region| {
                              total.checked_add(region.address.len())?
                                   .checked_add(region.size.len())
                          })
                          .ok_or(DriverError::InvalidDtb)?;
    if encoded_len != raw.len() {
        return Err(DriverError::InvalidDtb);
    }
    let mut result = Vec::new();
    let parsed = node.reg()
                     .ok_or(DriverError::InvalidDtb)?;
    for region in parsed {
        let base = region.starting_address as usize;
        let size = region.size
                         .ok_or(DriverError::InvalidDtb)?;
        if size == 0 ||
           base.checked_add(size)
               .is_none()
        {
            return Err(DriverError::InvalidDtb);
        }
        result.push(MmioRegion { base, size });
    }
    if result.is_empty() || raw.is_empty() {
        return Err(DriverError::InvalidDtb);
    }
    Ok(result)
}

fn phandle(node : fdt::node::FdtNode<'_, '_>) -> Option<u32> {
    property_u32(node, "phandle").or_else(|| property_u32(node, "linux,phandle"))
}

fn interrupt(node : fdt::node::FdtNode<'_, '_>) -> DriverResult<InterruptSpec> {
    let parent = node.interrupt_parent()
                     .ok_or(DriverError::InvalidDtb)?;
    let parent_phandle = phandle(parent).ok_or(DriverError::InvalidDtb)?;
    let cell_count = parent.interrupt_cells()
                           .ok_or(DriverError::InvalidDtb)?;
    if cell_count == 0 || cell_count > MAX_INTERRUPT_CELLS {
        return Err(DriverError::InvalidDtb);
    }
    let raw = node.property("interrupts")
                  .ok_or(DriverError::InvalidDtb)?
                  .value;
    if raw.len() != cell_count * 4 {
        return Err(DriverError::InvalidDtb);
    }
    let mut cells = [0; MAX_INTERRUPT_CELLS];
    for (index, cell) in cells[..cell_count].iter_mut()
                                            .enumerate()
    {
        *cell = read_be_u32(raw, index * 4).ok_or(DriverError::InvalidDtb)?;
    }
    Ok(InterruptSpec { parent_phandle,
                       cells,
                       cell_count : cell_count as u8 })
}

pub fn discover(fdt : &fdt::Fdt<'_>) -> DriverResult<BoardTopology> {
    let mut topology = BoardTopology { uarts : Vec::new(),
                                       interrupt_controllers : Vec::new(),
                                       mmc_hosts : Vec::new() };
    for node in fdt.all_nodes() {
        if !enabled(node)? {
            continue;
        }
        if has_compatible(node, "loongson,liointc-2.0") {
            let regs = regions(node)?;
            if regs.len() != 1 {
                return Err(DriverError::InvalidDtb);
            }
            let interrupt_cells = node.interrupt_cells()
                                      .ok_or(DriverError::InvalidDtb)?;
            if interrupt_cells == 0 || interrupt_cells > MAX_INTERRUPT_CELLS {
                return Err(DriverError::InvalidDtb);
            }
            topology.interrupt_controllers.push(InterruptControllerDescription {
                phandle : phandle(node).ok_or(DriverError::InvalidDtb)?,
                mmio : regs[0],
                interrupt_cells : interrupt_cells as u8,
            });
        } else if has_compatible(node, "ns16550a") {
            let regs = regions(node)?;
            if regs.len() != 1 {
                return Err(DriverError::InvalidDtb);
            }
            topology.uarts.push(UartDescription {
                mmio : regs[0],
                interrupt : interrupt(node)?,
                clock_hz : property_u32(node, "clock-frequency")
                    .ok_or(DriverError::InvalidDtb)?,
                register_shift : property_u32(node, "reg-shift").unwrap_or(0),
            });
        } else if has_compatible(node, "loongson,ls2k1000-mmc") {
            let regs = regions(node)?;
            if !(1..=2).contains(&regs.len()) {
                return Err(DriverError::InvalidDtb);
            }
            topology.mmc_hosts
                    .push(MmcDescription { controller_mmio : regs[0],
                                           auxiliary_mmio : regs.get(1).copied(),
                                           interrupt : interrupt(node)? });
        }
    }
    if topology.uarts
               .is_empty() ||
       topology.interrupt_controllers
               .is_empty() ||
       topology.mmc_hosts
               .is_empty()
    {
        return Err(DriverError::NotFound);
    }
    Ok(topology)
}
