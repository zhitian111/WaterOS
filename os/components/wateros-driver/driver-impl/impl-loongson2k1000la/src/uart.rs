//! Loongson 2K1000LA UART 探测与字符设备注册。

use api_v0::{DriverError, DriverResult};
use character::{self, RegisterLayout};
use common::dtb::{compatible_list, first_mmio_region, read_be_u32, read_fdt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UartDescription {
    pub mmio : api_v0::MmioRegion,
    pub layout : RegisterLayout,
}

pub fn register(uart : UartDescription) -> usize {
    character::register_uart_character_device(uart.mmio.base, uart.layout)
}

pub(crate) fn layout(reg_shift : Option<u32>,
                     reg_io_width : Option<u32>)
                     -> Option<RegisterLayout> {
    match (reg_shift.unwrap_or(0), reg_io_width.unwrap_or(1)) {
        (0, 1) => Some(RegisterLayout::Byte16550),
        (2, 4) => Some(RegisterLayout::DwApb32),
        _ => None,
    }
}

fn be32_property(node : &fdt::node::FdtNode<'_, '_>, name : &str) -> Option<u32> {
    node.property(name)
        .and_then(|p| read_be_u32(p.value, 0))
}

pub fn register_from_dtb(dtb_pa : usize) -> DriverResult<usize> {
    let fdt = read_fdt(dtb_pa)?;
    let mut registered = 0usize;
    for node in fdt.all_nodes() {
        let compatibles = compatible_list(&node);
        if !character::is_uart_compatible(&compatibles) {
            continue;
        }
        let node_name = node.name;
        let Some(layout) = layout(be32_property(&node, "reg-shift"),
                                  be32_property(&node, "reg-io-width"))
        else {
            log::warn!("[driver][2k1000] UART node {} has unsupported register layout",
                       node_name);
            continue;
        };
        let Some(mmio) = first_mmio_region(node) else {
            log::warn!("[driver][2k1000] UART node {} has no MMIO region",
                       node_name);
            continue;
        };
        let idx = register(UartDescription { mmio, layout });
        registered += 1;
        log::info!("[driver][2k1000] registered UART #{} node={} base={:#x} layout={:?}",
                   idx,
                   node_name,
                   mmio.base,
                   layout);
    }
    if registered == 0 {
        Err(DriverError::NotFound)
    } else {
        Ok(registered)
    }
}

/// 纯函数自检：验证已知寄存器布局映射。
pub fn test() {
    assert_eq!(layout(None, None),
               Some(RegisterLayout::Byte16550));
    assert_eq!(layout(Some(0), Some(1)),
               Some(RegisterLayout::Byte16550));
    assert_eq!(layout(Some(2), Some(4)),
               Some(RegisterLayout::DwApb32));
    assert_eq!(layout(Some(2), Some(1)), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_known_register_layout_pairs() { test(); }
}
