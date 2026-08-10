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
    pub clock_provider : MmcClockProvider,
    pub dma : Option<NamedResource>,
    pub bus_width : u8,
    pub pinctrl : Option<MmcPinctrlDescription>,
    pub card_detect : CardDetect,
    pub vmmc_supply : Option<SupplyDescription>,
    pub vqmmc_supply : Option<SupplyDescription>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmcPinctrlDescription {
    pub state_phandle : u32,
    pub provider : PinctrlProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinctrlProvider {
    /// The state matches upstream `sdio -> sdio` and `pwm2 -> gpio`.
    /// Register writes remain UNVERIFIED_ON_HARDWARE and are not implemented.
    Loongson2k { mmio : MmioRegion },
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcClockProvider {
    /// Topology evidence only. Register semantics are UNVERIFIED_ON_HARDWARE.
    Loongson2k { mmio : MmioRegion, reference_hz : u32 },
    Unsupported { phandle : u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupplyProvider {
    Fixed {
        control : FixedSupplyControl,
        always_on : bool,
        boot_on : bool,
    },
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedSupplyControl {
    /// Linux's regulator-fixed driver exposes empty ops without an enable GPIO.
    None,
    /// Control exists, but WaterOS deliberately has no rail-write path yet.
    Gpio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupplyDescription {
    /// Provider identity does not prove the rail's live electrical state.
    pub phandle : u32,
    pub provider : SupplyProvider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DmaControllerDescription {
    pub phandle : u32,
    pub mmio : MmioRegion,
    pub interrupt : InterruptSpec,
    pub clock : NamedResource,
    pub channel_cells : u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkDescription {
    pub bus : u8,
    pub device : u8,
    pub function : u8,
    pub interrupts : Vec<InterruptSpec>,
    pub interrupt_names : Vec<String>,
    pub phy_mode : Option<String>,
    pub phy_handle : Option<u32>,
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
    Gpio(GpioLineDescription),
    Broken,
    NonRemovable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioProvider {
    /// Register semantics are topology evidence and UNVERIFIED_ON_HARDWARE.
    Loongson2k1000 { mmio : MmioRegion, ngpios : u8 },
    Unsupported { phandle : u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpioLineDescription {
    pub specifier : ResourceSpecifier,
    pub provider : GpioProvider,
    pub pin : u8,
    pub active_low : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardTopology {
    pub uarts : Vec<UartDescription>,
    pub interrupt_controllers : Vec<InterruptControllerDescription>,
    pub mmc_hosts : Vec<MmcDescription>,
    pub dma_controllers : Vec<DmaControllerDescription>,
    pub networks : Vec<NetworkDescription>,
}

/// Coarse capability state exposed to bring-up diagnostics. Discovery is not
/// activation: MMC/DMA remain deferred until their hardware contracts are
/// verified on the target board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityState {
    Discovered,
    DeferredActivation,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardCapabilitySnapshot {
    pub uart_count : usize,
    pub irq_controller_count : usize,
    pub mmc_count : usize,
    pub dma_controller_count : usize,
    pub uart : CapabilityState,
    pub irq : CapabilityState,
    pub mmc : CapabilityState,
    pub dma : CapabilityState,
    pub network : CapabilityState,
    pub input : CapabilityState,
}

impl BoardTopology {
    pub fn capability_snapshot(&self) -> BoardCapabilitySnapshot {
        let state = |count : usize, deferred : bool| {
            if count == 0 {
                CapabilityState::Unsupported
            } else if deferred {
                CapabilityState::DeferredActivation
            } else {
                CapabilityState::Discovered
            }
        };
        BoardCapabilitySnapshot {
            uart_count : self.uarts.len(),
            irq_controller_count : self.interrupt_controllers.len(),
            mmc_count : self.mmc_hosts.len(),
            dma_controller_count : self.dma_controllers.len(),
            uart : state(self.uarts.len(), true),
            irq : state(self.interrupt_controllers.len(), true),
            mmc : state(self.mmc_hosts.len(), true),
            dma : state(self.dma_controllers.len(), true),
            // No LA DTB network/input parser or target-board implementation is
            // registered yet; do not infer support from unrelated QEMU drivers.
            network : state(self.networks.len(), true),
            input : CapabilityState::Unsupported,
        }
    }
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

fn pci_function(node : fdt::node::FdtNode<'_, '_>) -> DriverResult<(u8, u8, u8)> {
    let raw = node.property("reg").ok_or(DriverError::InvalidDtb)?.value;
    if raw.len() < 20 || raw.len() % 4 != 0 { return Err(DriverError::InvalidDtb); }
    let first = read_be_u32(raw, 0).ok_or(DriverError::InvalidDtb)?;
    Ok(((first >> 16) as u8, ((first >> 11) & 0x1f) as u8, ((first >> 8) & 7) as u8))
}

fn optional_string(node : fdt::node::FdtNode<'_, '_>, name : &str)
                    -> DriverResult<Option<String>> {
    let Some(raw) = node.property(name).map(|property| property.value) else { return Ok(None); };
    let value = raw.strip_suffix(&[0]).ok_or(DriverError::InvalidDtb)?;
    Ok(Some(String::from(core::str::from_utf8(value).map_err(|_| DriverError::InvalidDtb)?)))
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

fn supply_description(fdt : &fdt::Fdt<'_>,
                      node : fdt::node::FdtNode<'_, '_>,
                      property : &str)
                      -> DriverResult<Option<SupplyDescription>> {
    let Some(raw) = node.property(property)
                        .map(|property| property.value)
    else {
        return Ok(None);
    };
    if raw.len() != 4 {
        return Err(DriverError::InvalidDtb);
    }
    let phandle = read_be_u32(raw, 0).ok_or(DriverError::InvalidDtb)?;
    let provider_node = fdt.find_phandle(phandle)
                           .ok_or(DriverError::InvalidDtb)?;
    let provider = if has_compatible(provider_node, "regulator-fixed") {
        let gpio = provider_node.property("gpio").is_some();
        let gpios = provider_node.property("gpios").is_some();
        if gpio && gpios {
            return Err(DriverError::InvalidDtb);
        }
        SupplyProvider::Fixed {
            control : if gpio || gpios { FixedSupplyControl::Gpio } else {
                FixedSupplyControl::None
            },
            always_on : boolean_property(provider_node, "regulator-always-on")?,
            boot_on : boolean_property(provider_node, "regulator-boot-on")?,
        }
    } else {
        SupplyProvider::Unsupported
    };
    Ok(Some(SupplyDescription { phandle, provider }))
}

fn mmc_pinctrl(fdt : &fdt::Fdt<'_>,
               node : fdt::node::FdtNode<'_, '_>)
               -> DriverResult<Option<MmcPinctrlDescription>> {
    let (Some(names), Some(states)) = (node.property("pinctrl-names"),
                                       node.property("pinctrl-0"))
    else {
        if node.property("pinctrl-names").is_some() || node.property("pinctrl-0").is_some() {
            return Err(DriverError::InvalidDtb);
        }
        return Ok(None);
    };
    if names.value != b"default\0" || states.value.len() != 4 {
        return Err(DriverError::InvalidDtb);
    }
    let state_phandle = read_be_u32(states.value, 0).ok_or(DriverError::InvalidDtb)?;
    let state = fdt.find_phandle(state_phandle)
                   .ok_or(DriverError::InvalidDtb)?;
    let mut provider = None;
    for candidate in fdt.all_nodes() {
        if has_compatible(candidate, "loongson,ls2k-pinctrl") &&
           candidate.children()
                    .any(|child| phandle(child) == Some(state_phandle))
        {
            if provider.is_some() || !enabled(candidate)? {
                return Err(DriverError::InvalidDtb);
            }
            provider = Some(candidate);
        }
    }
    let Some(provider) = provider else {
        return Ok(Some(MmcPinctrlDescription { state_phandle,
                                               provider : PinctrlProvider::Unsupported }));
    };
    let regs = regions(provider)?;
    if regs.len() != 1 || regs[0].base == 0 || regs[0].base % 4 != 0 || regs[0].size < 0x18 {
        return Err(DriverError::InvalidDtb);
    }
    let mut sdio = false;
    let mut card_detect_gpio = false;
    for mapping in state.children() {
        let groups = string_list(mapping, "groups")?;
        let functions = string_list(mapping, "function")?;
        let pair = (groups.as_slice(), functions.as_slice());
        if pair == (["sdio"].as_slice(), ["sdio"].as_slice()) && !sdio {
            sdio = true;
        } else if pair == (["pwm2"].as_slice(), ["gpio"].as_slice()) && !card_detect_gpio {
            card_detect_gpio = true;
        } else {
            return Err(DriverError::InvalidDtb);
        }
    }
    if !sdio || !card_detect_gpio {
        return Err(DriverError::InvalidDtb);
    }
    Ok(Some(MmcPinctrlDescription {
        state_phandle,
        provider : PinctrlProvider::Loongson2k { mmio : regs[0] },
    }))
}

fn mmc_clock_provider(fdt : &fdt::Fdt<'_>,
                      clock : &NamedResource)
                      -> DriverResult<MmcClockProvider> {
    let phandle = clock.specifier.provider_phandle;
    let provider = fdt.find_phandle(phandle)
                      .ok_or(DriverError::InvalidDtb)?;
    if !has_compatible(provider, "loongson,ls2k-clk") {
        return Ok(MmcClockProvider::Unsupported { phandle });
    }
    if clock.specifier.args.as_slice() != [12] {
        return Err(DriverError::InvalidDtb);
    }
    let regs = regions(provider)?;
    if regs.len() != 1 || regs[0].base == 0 || regs[0].size < 0x58 {
        return Err(DriverError::InvalidDtb);
    }
    if string_list(provider, "clock-names")? != ["ref_100m"] {
        return Err(DriverError::InvalidDtb);
    }
    let references = resource_specifiers(fdt, provider, "clocks", "#clock-cells")?;
    if references.len() != 1 || !references[0].args.is_empty() {
        return Err(DriverError::InvalidDtb);
    }
    let reference = fdt.find_phandle(references[0].provider_phandle)
                       .ok_or(DriverError::InvalidDtb)?;
    if !has_compatible(reference, "fixed-clock") {
        return Err(DriverError::InvalidDtb);
    }
    let reference_hz = property_u32(reference, "clock-frequency")
        .filter(|rate| *rate != 0)
        .ok_or(DriverError::InvalidDtb)?;
    Ok(MmcClockProvider::Loongson2k { mmio : regs[0], reference_hz })
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
        let specifier = specifiers.remove(0);
        if specifier.args.len() != 2 || specifier.args[1] > 1 {
            return Err(DriverError::InvalidDtb);
        }
        let provider_node = fdt.find_phandle(specifier.provider_phandle)
                               .ok_or(DriverError::InvalidDtb)?;
        let provider = if has_compatible(provider_node, "loongson,ls2k1000-gpio") &&
                          has_compatible(provider_node, "loongson,ls2k-gpio")
        {
            let regs = regions(provider_node)?;
            let ngpios = property_u32(provider_node, "ngpios")
                .filter(|count| (1..=64).contains(count))
                .ok_or(DriverError::InvalidDtb)?;
            if regs.len() != 1 || regs[0].base == 0 || regs[0].base % 8 != 0 ||
               regs[0].size < 0x28 || specifier.args[0] >= ngpios
            {
                return Err(DriverError::InvalidDtb);
            }
            GpioProvider::Loongson2k1000 { mmio : regs[0], ngpios : ngpios as u8 }
        } else {
            GpioProvider::Unsupported { phandle : specifier.provider_phandle }
        };
        Ok(CardDetect::Gpio(GpioLineDescription { pin : specifier.args[0] as u8,
                                                  active_low : specifier.args[1] == 1,
                                                  specifier,
                                                  provider }))
    } else {
        Ok(CardDetect::Native)
    }
}

pub fn discover(fdt : &fdt::Fdt<'_>) -> DriverResult<BoardTopology> {
    let mut topology = BoardTopology { uarts : Vec::new(),
                                       interrupt_controllers : Vec::new(),
                                       mmc_hosts : Vec::new(),
                                       dma_controllers : Vec::new(),
                                       networks : Vec::new() };
    for node in fdt.all_nodes() {
        if !enabled(node)? {
            continue;
        }
        if node.name.starts_with("ethernet@") {
            let (bus, device, function) = pci_function(node)?;
            if device != 3 { continue; }
            let interrupts = interrupt_specs(node)?;
            if interrupts.is_empty() { return Err(DriverError::InvalidDtb); }
            let interrupt_names = match node.property("interrupt-names") {
                Some(_) => string_list(node, "interrupt-names")?.into_iter().map(String::from).collect(),
                None => Vec::new(),
            };
            topology.networks.push(NetworkDescription {
                bus, device, function, interrupts, interrupt_names,
                phy_mode : optional_string(node, "phy-mode")?,
                phy_handle : property_u32(node, "phy-handle"),
            });
        } else if has_compatible(node, "loongson,liointc-2.0") {
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
        } else if has_compatible(node, "loongson,ls2k1000-apbdma") {
            let regs = regions(node)?;
            if regs.len() != 1 || regs[0].size != 8 || regs[0].base == 0 || regs[0].base % 4 != 0 {
                return Err(DriverError::InvalidDtb);
            }
            let clocks = named_resources(fdt,
                                         node,
                                         "clocks",
                                         "clock-names",
                                         "#clock-cells",
                                         false)?;
            if clocks.len() != 1 || property_u32(node, "#dma-cells") != Some(1) {
                return Err(DriverError::InvalidDtb);
            }
            topology.dma_controllers
                    .push(DmaControllerDescription {
                        phandle : phandle(node).ok_or(DriverError::InvalidDtb)?,
                        mmio : regs[0],
                        interrupt : interrupt(node)?,
                        clock : clocks.into_iter().next().ok_or(DriverError::InvalidDtb)?,
                        channel_cells : 1,
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
            let clocks = named_resources(fdt,
                                         node,
                                         "clocks",
                                         "clock-names",
                                         "#clock-cells",
                                         false)?;
            if clocks.len() != 1 {
                return Err(DriverError::InvalidDtb);
            }
            let clock_provider = mmc_clock_provider(fdt, &clocks[0])?;
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
                                           clock_provider,
                                           dma,
                                           bus_width : bus_width as u8,
                                           pinctrl : mmc_pinctrl(fdt, node)?,
                                           card_detect : mmc_card_detect(fdt, node)?,
                                           vmmc_supply : supply_description(fdt,
                                                                            node,
                                                                            "vmmc-supply")?,
                                           vqmmc_supply : supply_description(fdt,
                                                                             node,
                                                                             "vqmmc-supply")? });
        }
    }
    if topology.uarts
               .is_empty() ||
       topology.interrupt_controllers
               .is_empty() ||
       topology.dma_controllers
               .is_empty() ||
       topology.mmc_hosts
               .is_empty()
    {
        return Err(DriverError::NotFound);
    }
    Ok(topology)
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use super::*;

    #[test]
    fn capability_snapshot_separates_discovery_from_activation() {
        let topology = BoardTopology { uarts : vec![UartDescription {
                                                  mmio : MmioRegion { base : 0x1000, size : 0x100 },
                                                  interrupt : InterruptSpec {
                                                      parent_phandle : 1,
                                                      cells : [0; 4],
                                                      cell_count : 2,
                                                  },
                                                  clock_hz : 125_000_000,
                                                  register_shift : 0,
                                              }],
                                  interrupt_controllers : Vec::new(),
                                  mmc_hosts : Vec::new(),
                                  dma_controllers : Vec::new(),
                                  networks : Vec::new() };
        let snapshot = topology.capability_snapshot();
        assert_eq!(snapshot.uart_count, 1);
        assert_eq!(snapshot.uart, CapabilityState::DeferredActivation);
        assert_eq!(snapshot.irq, CapabilityState::Unsupported);
        assert_eq!(snapshot.network, CapabilityState::Unsupported);
        assert_eq!(snapshot.input, CapabilityState::Unsupported);
    }
}
