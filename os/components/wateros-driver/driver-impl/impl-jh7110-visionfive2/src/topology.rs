use crate::{
    mmc::{MmcHostDescription, ResourceSpecifier, SysregField},
    plic::{parse_contexts, PlicDescription},
    uart::{self, UartDescription},
};
use alloc::{format, string::String, vec::Vec};
use api_v0::{DriverError, DriverResult};
use character::is_uart_compatible;
use common::dtb;
use spin::Mutex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoardTopology {
    pub console_uart : Option<UartDescription>,
    pub plic : Option<PlicDescription>,
    pub mmc_hosts : Vec<MmcHostDescription>,
}
static TOPOLOGY : Mutex<Option<BoardTopology>> = Mutex::new(None);
pub(crate) fn store(topology : BoardTopology) { *TOPOLOGY.lock() = Some(topology); }
pub fn with_topology<R>(f : impl FnOnce(Option<&BoardTopology>) -> R) -> R {
    let guard = TOPOLOGY.lock();
    f(guard.as_ref())
}

fn be32_property(node : &fdt::node::FdtNode<'_, '_>, name : &str) -> Option<u32> {
    node.property(name)
        .and_then(|p| dtb::read_be_u32(p.value, 0))
}
fn is_plic(compatibles : &[alloc::string::String]) -> bool {
    compatibles.iter()
               .any(|c| {
                   matches!(c.as_str(),
                            "riscv,plic0" | "sifive,plic-1.0.0")
               })
}
fn is_enabled(node : &fdt::node::FdtNode<'_, '_>) -> bool {
    node.property("status")
        .and_then(|property| core::str::from_utf8(property.value).ok())
        .is_none_or(|status| {
            matches!(status.trim_end_matches('\0'),
                     "okay" | "ok")
        })
}
fn bus_width(value : Option<u32>) -> DriverResult<u8> {
    match value.unwrap_or(1) {
        width @ (1 | 4 | 8) => Ok(width as u8),
        _ => Err(DriverError::InvalidDtb),
    }
}

fn string_list(node : &fdt::node::FdtNode<'_, '_>, property : &str)
    -> DriverResult<Vec<String>>
{
    let bytes = node.property(property).ok_or(DriverError::InvalidDtb)?.value;
    if bytes.is_empty() || bytes.last() != Some(&0) {
        return Err(DriverError::InvalidDtb);
    }
    bytes[..bytes.len() - 1]
        .split(|byte| *byte == 0)
        .map(|value| {
            core::str::from_utf8(value)
                .ok()
                .filter(|value| !value.is_empty())
                .map(String::from)
                .ok_or(DriverError::InvalidDtb)
        })
        .collect()
}

fn phandle_specifiers(fdt : &fdt::Fdt<'_>,
                      node : &fdt::node::FdtNode<'_, '_>,
                      property : &str,
                      provider_cells_property : &str)
                      -> DriverResult<Vec<ResourceSpecifier>> {
    let bytes = node.property(property).ok_or(DriverError::InvalidDtb)?.value;
    if bytes.is_empty() || bytes.len() % 4 != 0 {
        return Err(DriverError::InvalidDtb);
    }
    let mut offset = 0;
    let mut result = Vec::new();
    while offset < bytes.len() {
        let provider = dtb::read_be_u32(bytes, offset).ok_or(DriverError::InvalidDtb)?;
        offset += 4;
        let provider_node = fdt.find_phandle(provider).ok_or(DriverError::InvalidDtb)?;
        let cells = be32_property(&provider_node, provider_cells_property)
            .filter(|cells| *cells <= 8)
            .ok_or(DriverError::InvalidDtb)? as usize;
        let args_bytes = cells.checked_mul(4).ok_or(DriverError::InvalidDtb)?;
        let end = offset.checked_add(args_bytes)
                        .filter(|end| *end <= bytes.len())
                        .ok_or(DriverError::InvalidDtb)?;
        let mut args = Vec::with_capacity(cells);
        while offset < end {
            args.push(dtb::read_be_u32(bytes, offset).ok_or(DriverError::InvalidDtb)?);
            offset += 4;
        }
        result.push(ResourceSpecifier { provider, args });
    }
    Ok(result)
}

fn named_specifier(names : &[String],
                   specifiers : &[ResourceSpecifier],
                   wanted : &str)
                   -> DriverResult<ResourceSpecifier> {
    if names.len() != specifiers.len() ||
       names.iter().filter(|name| name.as_str() == wanted).count() != 1
    {
        return Err(DriverError::InvalidDtb);
    }
    names.iter().position(|name| name == wanted)
         .and_then(|index| specifiers.get(index))
         .cloned()
         .ok_or(DriverError::InvalidDtb)
}

fn sysreg_field(fdt : &fdt::Fdt<'_>, node : &fdt::node::FdtNode<'_, '_>)
    -> DriverResult<Option<SysregField>>
{
    let Some(property) = node.property("starfive,sysreg") else { return Ok(None); };
    if property.value.len() != 16 {
        return Err(DriverError::InvalidDtb);
    }
    let provider = dtb::read_be_u32(property.value, 0).ok_or(DriverError::InvalidDtb)?;
    fdt.find_phandle(provider).ok_or(DriverError::InvalidDtb)?;
    let offset = dtb::read_be_u32(property.value, 4).ok_or(DriverError::InvalidDtb)?;
    let shift = dtb::read_be_u32(property.value, 8).ok_or(DriverError::InvalidDtb)?;
    let mask = dtb::read_be_u32(property.value, 12).ok_or(DriverError::InvalidDtb)?;
    if offset % 4 != 0 || shift >= 32 || mask == 0 || mask & ((1u32 << shift) - 1) != 0 {
        return Err(DriverError::InvalidDtb);
    }
    Ok(Some(SysregField { provider, offset, shift: shift as u8, mask }))
}

fn cpu_interrupt_controllers(fdt : &fdt::Fdt<'_>) -> Vec<(u32, usize)> {
    let mut controllers = Vec::new();
    for cpu in fdt.all_nodes() {
        let is_cpu = cpu.property("device_type")
                        .and_then(|property| core::str::from_utf8(property.value).ok())
                        .is_some_and(|kind| kind.trim_end_matches('\0') == "cpu");
        if !is_cpu {
            continue;
        }
        let Some(hart_id) = cpu.reg()
                               .and_then(|mut regions| regions.next())
                               .map(|region| region.starting_address as usize)
        else {
            continue;
        };
        let path = format!("/cpus/{}/interrupt-controller",
                           cpu.name);
        let Some(controller) = fdt.find_node(&path) else {
            continue;
        };
        let phandle = be32_property(&controller, "phandle").or_else(|| {
                                                               be32_property(&controller,
                                                                             "linux,phandle")
                                                           });
        if let Some(phandle) = phandle {
            controllers.push((phandle, hart_id));
        }
    }
    controllers
}

pub fn discover(dtb_pa : usize) -> DriverResult<BoardTopology> {
    let fdt = dtb::read_fdt(dtb_pa)?;
    // Production firmware commonly appends `:115200n8`; the fdt crate's
    // `Chosen::stdout` helper does not strip that suffix before alias lookup.
    let chosen = fdt.find_node("/chosen")
                    .and_then(|node| node.property("stdout-path"))
                    .and_then(|property| core::str::from_utf8(property.value).ok())
                    .map(|path| {
                        path.trim_end_matches('\0')
                            .split(':')
                            .next()
                            .unwrap_or(path)
                    })
                    .and_then(|path| fdt.find_node(path));
    let chosen_name = chosen.map(|n| n.name);
    let cpu_controllers = cpu_interrupt_controllers(&fdt);
    let mut result = BoardTopology::default();
    for node in fdt.all_nodes() {
        let compatibles = dtb::compatible_list(&node);
        if compatibles.iter()
                      .any(|compatible| compatible == "starfive,jh7110-mmc") &&
           is_enabled(&node)
        {
            let mmio = dtb::first_mmio_region(node).ok_or(DriverError::InvalidDtb)?;
            let irq = node.property("interrupts")
                          .and_then(|property| dtb::read_be_u32(property.value, 0))
                          .ok_or(DriverError::InvalidDtb)?;
            let bus_width = bus_width(be32_property(&node, "bus-width"))?;
            let clock_names = string_list(&node, "clock-names")?;
            let clocks = phandle_specifiers(&fdt, &node, "clocks", "#clock-cells")?;
            let resets = phandle_specifiers(&fdt, &node, "resets", "#reset-cells")?;
            if resets.len() != 1 {
                return Err(DriverError::InvalidDtb);
            }
            result.mmc_hosts
                  .push(MmcHostDescription { mmio,
                                             irq,
                                             bus_width,
                                             max_frequency_hz : be32_property(&node,
                                                                              "max-frequency"),
                                             fifo_depth : be32_property(&node, "fifo-depth"),
                                             non_removable : node.property("non-removable")
                                                                 .is_some(),
                                             biu_clock : named_specifier(&clock_names,
                                                                           &clocks,
                                                                           "biu")?,
                                             ciu_clock : named_specifier(&clock_names,
                                                                           &clocks,
                                                                           "ciu")?,
                                             reset : resets.into_iter().next()
                                                           .ok_or(DriverError::InvalidDtb)?,
                                             sysreg : sysreg_field(&fdt, &node)? });
        }
        if is_plic(&compatibles) {
            let mmio = dtb::first_mmio_region(node).ok_or(DriverError::InvalidDtb)?;
            let sources = be32_property(&node, "riscv,ndev").ok_or(DriverError::InvalidDtb)?;
            let raw = node.property("interrupts-extended")
                          .ok_or(DriverError::InvalidDtb)?
                          .value;
            let mut contexts = parse_contexts(raw)?;
            for context in &mut contexts {
                context.hart_id =
                    cpu_controllers.iter()
                                   .find(|(phandle, _)| *phandle == context.interrupt_controller)
                                   .map(|(_, hart)| *hart);
            }
            result.plic = Some(PlicDescription { mmio,
                                                 sources,
                                                 contexts });
        }
        if result.console_uart
                 .is_none() &&
           chosen_name == Some(node.name) &&
           is_uart_compatible(&compatibles)
        {
            let mmio = dtb::first_mmio_region(node).ok_or(DriverError::InvalidDtb)?;
            let layout =
                uart::layout(be32_property(&node, "reg-shift"),
                             be32_property(&node, "reg-io-width")).ok_or(DriverError::Unsupported)?;
            result.console_uart = Some(UartDescription { mmio, layout });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_supported_layouts_and_plic_names() {
        assert_eq!(uart::layout(Some(2), Some(4)),
                   Some(character::RegisterLayout::DwApb32));
        assert_eq!(uart::layout(Some(1), Some(4)), None);
        assert!(is_plic(&alloc::vec!["starfive,jh7110-plic".into(),
                                     "sifive,plic-1.0.0".into()]));
        assert_eq!(bus_width(None), Ok(1));
        assert_eq!(bus_width(Some(8)), Ok(8));
        assert_eq!(bus_width(Some(260)), Err(DriverError::InvalidDtb));
    }
}
