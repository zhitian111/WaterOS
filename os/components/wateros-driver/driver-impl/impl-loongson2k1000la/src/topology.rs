use alloc::{string::String, vec::Vec};

use api_v0::{DriverError, DriverResult, MmioRegion};
use common::dtb::read_be_u32;

const MAX_INTERRUPT_CELLS : usize = 4;
const MAX_RESOURCE_CELLS : usize = 8;

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
    pub phandle : Option<u32>,
    pub main_mmio : MmioRegion,
    pub core_isr : Vec<MmioRegion>,
    pub interrupt_cells : u8,
    pub parent_interrupts : [Option<InterruptSpec>; 4],
    pub parent_source_maps : [u32; 4],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcDescription {
    pub controller_mmio : MmioRegion,
    pub auxiliary_mmio : Option<MmioRegion>,
    pub interrupt : InterruptSpec,
    pub clocks : Vec<NamedResource>,
    pub dma : Option<NamedResource>,
    pub bus_width : u8,
    pub card_detect : CardDetect,
    pub vmmc_supply : Option<u32>,
    pub vqmmc_supply : Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecifier {
    pub provider_phandle : u32,
    pub args : Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedResource {
    pub name : Option<String>,
    pub specifier : ResourceSpecifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardDetect {
    Native,
    Gpio(ResourceSpecifier),
    Broken,
    NonRemovable,
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

fn string_list<'b, 'a : 'b>(node : fdt::node::FdtNode<'b, 'a>,
                            name : &str)
                            -> DriverResult<Vec<&'a str>> {
    let raw = node.property(name)
                  .ok_or(DriverError::InvalidDtb)?
                  .value;
    if raw.is_empty() || !raw.ends_with(&[0]) {
        return Err(DriverError::InvalidDtb);
    }
    raw[..raw.len() - 1].split(|byte| *byte == 0)
                        .map(|item| {
                            if item.is_empty() {
                                return Err(DriverError::InvalidDtb);
                            }
                            core::str::from_utf8(item).map_err(|_| DriverError::InvalidDtb)
                        })
                        .collect()
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

fn interrupt_specs(node : fdt::node::FdtNode<'_, '_>) -> DriverResult<Vec<InterruptSpec>> {
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
    let stride = cell_count * 4;
    if raw.is_empty() || raw.len() % stride != 0 {
        return Err(DriverError::InvalidDtb);
    }
    let mut specs = Vec::new();
    for encoded in raw.chunks_exact(stride) {
        let mut cells = [0; MAX_INTERRUPT_CELLS];
        for (index, cell) in cells[..cell_count].iter_mut()
                                                .enumerate()
        {
            *cell = read_be_u32(encoded, index * 4).ok_or(DriverError::InvalidDtb)?;
        }
        specs.push(InterruptSpec { parent_phandle,
                                   cells,
                                   cell_count : cell_count as u8 });
    }
    Ok(specs)
}

fn interrupt(node : fdt::node::FdtNode<'_, '_>) -> DriverResult<InterruptSpec> {
    let mut specs = interrupt_specs(node)?;
    if specs.len() != 1 {
        return Err(DriverError::InvalidDtb);
    }
    Ok(specs.remove(0))
}

fn parse_parent_routes(node : fdt::node::FdtNode<'_, '_>)
                       -> DriverResult<([Option<InterruptSpec>; 4], [u32; 4])> {
    let names = string_list(node, "interrupt-names")?;
    let specs = interrupt_specs(node)?;
    if names.is_empty() || names.len() != specs.len() || names.len() > 4 {
        return Err(DriverError::InvalidDtb);
    }
    let mut parents : [Option<InterruptSpec>; 4] = core::array::from_fn(|_| None);
    for (name, spec) in names.iter()
                             .zip(specs)
    {
        let line = match *name {
            "int0" => 0,
            "int1" => 1,
            "int2" => 2,
            "int3" => 3,
            _ => return Err(DriverError::InvalidDtb),
        };
        if parents[line].replace(spec)
                        .is_some()
        {
            return Err(DriverError::InvalidDtb);
        }
    }

    let raw = node.property("loongson,parent_int_map")
                  .ok_or(DriverError::InvalidDtb)?
                  .value;
    if raw.len() != 16 {
        return Err(DriverError::InvalidDtb);
    }
    let mut maps = [0; 4];
    let mut covered = 0u32;
    for (line, map) in maps.iter_mut()
                           .enumerate()
    {
        *map = read_be_u32(raw, line * 4).ok_or(DriverError::InvalidDtb)?;
        if covered & *map != 0 || (*map != 0 && parents[line].is_none()) {
            return Err(DriverError::InvalidDtb);
        }
        covered |= *map;
    }
    if covered != u32::MAX {
        return Err(DriverError::InvalidDtb);
    }
    Ok((parents, maps))
}

fn resource_specifiers(fdt : &fdt::Fdt<'_>,
                       node : fdt::node::FdtNode<'_, '_>,
                       property : &str,
                       provider_cells : &str)
                       -> DriverResult<Vec<ResourceSpecifier>> {
    let raw = node.property(property)
                  .ok_or(DriverError::InvalidDtb)?
                  .value;
    if raw.is_empty() || raw.len() % 4 != 0 {
        return Err(DriverError::InvalidDtb);
    }
    let mut offset = 0usize;
    let mut result = Vec::new();
    while offset < raw.len() {
        let provider_phandle = read_be_u32(raw, offset).ok_or(DriverError::InvalidDtb)?;
        offset += 4;
        let provider = fdt.find_phandle(provider_phandle)
                          .ok_or(DriverError::InvalidDtb)?;
        let cell_count =
            property_u32(provider, provider_cells).ok_or(DriverError::InvalidDtb)? as usize;
        if cell_count > MAX_RESOURCE_CELLS {
            return Err(DriverError::InvalidDtb);
        }
        let byte_count = cell_count.checked_mul(4)
                                   .ok_or(DriverError::InvalidDtb)?;
        if offset.checked_add(byte_count)
                 .filter(|end| *end <= raw.len())
                 .is_none()
        {
            return Err(DriverError::InvalidDtb);
        }
        let mut args = Vec::with_capacity(cell_count);
        for _ in 0..cell_count {
            args.push(read_be_u32(raw, offset).ok_or(DriverError::InvalidDtb)?);
            offset += 4;
        }
        result.push(ResourceSpecifier { provider_phandle,
                                        args });
    }
    Ok(result)
}

fn named_resources(fdt : &fdt::Fdt<'_>,
                   node : fdt::node::FdtNode<'_, '_>,
                   property : &str,
                   names_property : &str,
                   provider_cells : &str,
                   names_required : bool)
                   -> DriverResult<Vec<NamedResource>> {
    let specifiers = resource_specifiers(fdt, node, property, provider_cells)?;
    let names = match node.property(names_property) {
        Some(_) => Some(string_list(node, names_property)?),
        None if names_required => return Err(DriverError::InvalidDtb),
        None => None,
    };
    if names.as_ref()
            .is_some_and(|names| names.len() != specifiers.len())
    {
        return Err(DriverError::InvalidDtb);
    }
    Ok(specifiers.into_iter()
                 .enumerate()
                 .map(|(index, specifier)| NamedResource { name : names.as_ref()
                                                                       .map(|names| {
                                                                           String::from(names
                                                                                            [index])
                                                                       }),
                                                           specifier })
                 .collect())
}

fn supply_phandle(fdt : &fdt::Fdt<'_>,
                  node : fdt::node::FdtNode<'_, '_>,
                  property : &str)
                  -> DriverResult<Option<u32>> {
    let Some(raw) = node.property(property)
                        .map(|property| property.value)
    else {
        return Ok(None);
    };
    if raw.len() != 4 {
        return Err(DriverError::InvalidDtb);
    }
    let phandle = read_be_u32(raw, 0).ok_or(DriverError::InvalidDtb)?;
    fdt.find_phandle(phandle)
       .ok_or(DriverError::InvalidDtb)?;
    Ok(Some(phandle))
}

fn boolean_property(node : fdt::node::FdtNode<'_, '_>, name : &str) -> DriverResult<bool> {
    match node.property(name) {
        Some(property)
            if property.value
                       .is_empty() =>
        {
            Ok(true)
        }
        Some(_) => Err(DriverError::InvalidDtb),
        None => Ok(false),
    }
}

fn mmc_card_detect(fdt : &fdt::Fdt<'_>,
                   node : fdt::node::FdtNode<'_, '_>)
                   -> DriverResult<CardDetect> {
    let non_removable = boolean_property(node, "non-removable")?;
    let broken = boolean_property(node, "broken-cd")?;
    let gpio = node.property("cd-gpios")
                   .is_some();
    if non_removable as u8 + broken as u8 + gpio as u8 > 1 {
        return Err(DriverError::InvalidDtb);
    }
    if non_removable {
        Ok(CardDetect::NonRemovable)
    } else if broken {
        Ok(CardDetect::Broken)
    } else if gpio {
        let mut specifiers = resource_specifiers(fdt, node, "cd-gpios", "#gpio-cells")?;
        if specifiers.len() != 1 {
            return Err(DriverError::InvalidDtb);
        }
        Ok(CardDetect::Gpio(specifiers.remove(0)))
    } else {
        Ok(CardDetect::Native)
    }
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
            let names = string_list(node, "reg-names")?;
            if !(2..=5).contains(&regs.len()) ||
               names.len() != regs.len() ||
               names.first()
                    .copied() !=
               Some("main")
            {
                return Err(DriverError::InvalidDtb);
            }
            for (core, name) in names[1..].iter()
                                          .enumerate()
            {
                let expected = match core {
                    0 => "isr0",
                    1 => "isr1",
                    2 => "isr2",
                    3 => "isr3",
                    _ => return Err(DriverError::InvalidDtb),
                };
                if *name != expected {
                    return Err(DriverError::InvalidDtb);
                }
            }
            let interrupt_cells = node.interrupt_cells()
                                      .ok_or(DriverError::InvalidDtb)?;
            if interrupt_cells == 0 || interrupt_cells > MAX_INTERRUPT_CELLS {
                return Err(DriverError::InvalidDtb);
            }
            let (parent_interrupts, parent_source_maps) = parse_parent_routes(node)?;
            topology.interrupt_controllers
                    .push(InterruptControllerDescription { phandle : phandle(node),
                                                           main_mmio : regs[0],
                                                           core_isr : Vec::from(&regs[1..]),
                                                           interrupt_cells : interrupt_cells
                                                                             as u8,
                                                           parent_interrupts,
                                                           parent_source_maps });
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
            let clocks = named_resources(fdt,
                                         node,
                                         "clocks",
                                         "clock-names",
                                         "#clock-cells",
                                         false)?;
            if clocks.len() != 1 {
                return Err(DriverError::InvalidDtb);
            }
            let dma = match (node.property("dmas"), node.property("dma-names")) {
                (None, None) => None,
                (Some(_), Some(_)) => {
                    let mut resources = named_resources(fdt,
                                                        node,
                                                        "dmas",
                                                        "dma-names",
                                                        "#dma-cells",
                                                        true)?;
                    if resources.len() != 1 ||
                       resources[0].name
                                   .as_deref() !=
                       Some("rx-tx")
                    {
                        return Err(DriverError::InvalidDtb);
                    }
                    Some(resources.remove(0))
                }
                _ => return Err(DriverError::InvalidDtb),
            };
            let bus_width = property_u32(node, "bus-width").unwrap_or(1);
            if !matches!(bus_width, 1 | 4 | 8) {
                return Err(DriverError::InvalidDtb);
            }
            topology.mmc_hosts
                    .push(MmcDescription { controller_mmio : regs[0],
                                           auxiliary_mmio : regs.get(1).copied(),
                                           interrupt : interrupt(node)?,
                                           clocks,
                                           dma,
                                           bus_width : bus_width as u8,
                                           card_detect : mmc_card_detect(fdt, node)?,
                                           vmmc_supply : supply_phandle(fdt,
                                                                        node,
                                                                        "vmmc-supply")?,
                                           vqmmc_supply : supply_phandle(fdt,
                                                                         node,
                                                                         "vqmmc-supply")? });
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
