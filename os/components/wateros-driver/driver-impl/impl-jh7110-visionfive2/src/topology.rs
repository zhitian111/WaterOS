use crate::{
    mmc::MmcHostDescription,
    plic::{parse_contexts, PlicDescription},
    uart::{self, UartDescription},
};
use alloc::{format, vec::Vec};
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
            result.mmc_hosts
                  .push(MmcHostDescription { mmio,
                                             irq,
                                             bus_width,
                                             max_frequency_hz : be32_property(&node,
                                                                              "max-frequency"),
                                             fifo_depth : be32_property(&node, "fifo-depth"),
                                             non_removable : node.property("non-removable")
                                                                 .is_some() });
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
