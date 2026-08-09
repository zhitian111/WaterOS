use crate::{
    plic::{parse_contexts, PlicDescription},
    uart::{self, UartDescription},
};
use api_v0::{DriverError, DriverResult};
use character::is_uart_compatible;
use common::dtb;
use spin::Mutex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoardTopology {
    pub console_uart : Option<UartDescription>,
    pub plic : Option<PlicDescription>,
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
    let mut result = BoardTopology::default();
    for node in fdt.all_nodes() {
        let compatibles = dtb::compatible_list(&node);
        if is_plic(&compatibles) {
            let mmio = dtb::first_mmio_region(node).ok_or(DriverError::InvalidDtb)?;
            let sources = be32_property(&node, "riscv,ndev").ok_or(DriverError::InvalidDtb)?;
            let raw = node.property("interrupts-extended")
                          .ok_or(DriverError::InvalidDtb)?
                          .value;
            result.plic = Some(PlicDescription { mmio,
                                                 sources,
                                                 contexts : parse_contexts(raw)? });
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
    }
}
