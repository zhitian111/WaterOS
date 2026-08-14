//! DTB 访问与节点解析原语。
//!
//! DTB 物理指针由 platform 唯一持有（见 `wateros-platform::init_when_boot`），
//! 本模块只提供按指针解析的只读助手；设备探测与注册等 transport 相关逻辑不在
//! 此处。

use alloc::{string::String, vec::Vec};

use api_v0::{DriverError, DriverResult, IrqLine, IrqSpec, MmioRegion};
use fdt::Fdt;

// `unsafe`：`dtb_pa` 指向的 DTB 在内核存活期内常驻且布局合法；返回的 `Fdt` 仅用于只读扫描。
pub fn read_fdt(dtb_pa: usize) -> DriverResult<Fdt<'static>> {
    if dtb_pa == 0 {
        return Err(DriverError::NotFound);
    }
    let fdt = unsafe { Fdt::from_ptr(dtb_pa as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
    Ok(fdt)
}

/// DTB 属性值为大端；`offset` 须对齐到 4 字节边界（此处由调用方保证长度）。
pub fn read_be_u32(raw: &[u8], offset: usize) -> Option<u32> {
    let bytes = raw.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// 取节点 `reg` 的第一段作为 MMIO 窗口；多段设备当前仅使用首段。
pub fn first_mmio_region(node: fdt::node::FdtNode<'_, '_>) -> Option<MmioRegion> {
    let mut regions = node.reg()?;
    let region = regions.next()?;
    let base = region.starting_address as usize;
    let size = region.size?;
    if size == 0 {
        return None;
    }
    Some(MmioRegion { base, size })
}

/// 仅覆盖「单 cell 中断号 + 可选 interrupt-parent」形态；PLIC/GPIO 复用等复杂描述返回 `None` 而非误解析。
pub fn parse_irq(node: &fdt::node::FdtNode<'_, '_>) -> Option<IrqLine> {
    let irq = node.property("interrupts")?.value;
    let irq_num = read_be_u32(irq, 0)?;
    let parent = node
        .property("interrupt-parent")
        .and_then(|p| read_be_u32(p.value, 0));
    Some(IrqLine {
        irq: irq_num,
        parent,
    })
}

/// 节点显式声明的 `interrupt-parent` phandle；未声明返回 `None`。
///
/// 注：DTB 规范允许沿父链继承 `interrupt-parent`，但 `fdt` crate 未暴露父节点
/// 引用；QEMU virt 设备节点均显式声明，继承语义留待 PCI `interrupt-map`（T03）
/// 处理。
pub fn interrupt_parent_phandle(node: &fdt::node::FdtNode<'_, '_>) -> Option<u32> {
    node.property("interrupt-parent")
        .and_then(|p| read_be_u32(p.value, 0))
}

/// 读取中断控制器节点声明的 `#interrupt-cells`；未解析到时按 DTB 默认值 1。
pub fn interrupt_cells_of(fdt: &Fdt<'static>, parent: Option<u32>) -> u32 {
    let Some(phandle) = parent else {
        return 1;
    };
    let Some(controller) = fdt.find_phandle(phandle) else {
        return 1;
    };
    controller
        .property("#interrupt-cells")
        .and_then(|p| read_be_u32(p.value, 0))
        .unwrap_or(1)
}

/// 解析节点 `interrupts` 的全部行（每行 `#interrupt-cells` 个 cells）。
///
/// 兼容原有 [`parse_irq`] 的单行单 cell 形态，并保留多行 / 多 cell 信息供 irq
/// domain 与后续 PCI `interrupt-map` 使用；属性缺失或格式非法返回空列表。
pub fn parse_irq_specs(fdt: &Fdt<'static>, node: &fdt::node::FdtNode<'_, '_>) -> Vec<IrqSpec> {
    let Some(raw) = node.property("interrupts").map(|p| p.value) else {
        return Vec::new();
    };
    let parent = interrupt_parent_phandle(node);
    let Some(cells) = usize::try_from(interrupt_cells_of(fdt, parent)).ok() else {
        return Vec::new();
    };
    if cells == 0 {
        return Vec::new();
    }
    let mut specs = Vec::new();
    let mut offset = 0usize;
    while offset
        .checked_add(cells * 4)
        .is_some_and(|end| end <= raw.len())
    {
        let mut line = Vec::with_capacity(cells);
        for index in 0..cells {
            match read_be_u32(raw, offset + index * 4) {
                Some(cell) => line.push(cell),
                None => return specs,
            }
        }
        specs.push(IrqSpec { parent, cells: line });
        offset += cells * 4;
    }
    specs
}

/// `interrupt-map-mask` 属性（bus 节点，如 PCI）的原始 cells。
pub fn interrupt_map_mask(node: &fdt::node::FdtNode<'_, '_>) -> Vec<u32> {
    let Some(raw) = node.property("interrupt-map-mask").map(|p| p.value) else {
        return Vec::new();
    };
    cells_from_raw(raw)
}

/// `interrupt-map` 属性原始 cells（bus 节点，如 PCI）；语义解码留待 irq domain 层。
pub fn interrupt_map_raw(node: &fdt::node::FdtNode<'_, '_>) -> Vec<u32> {
    let Some(raw) = node.property("interrupt-map").map(|p| p.value) else {
        return Vec::new();
    };
    cells_from_raw(raw)
}

fn cells_from_raw(raw: &[u8]) -> Vec<u32> {
    let mut cells = Vec::new();
    let mut offset = 0usize;
    while let Some(cell) = read_be_u32(raw, offset) {
        cells.push(cell);
        offset += 4;
    }
    cells
}

/// `compatible` 为以 `NUL` 分隔的 C 字符串序列；非法 UTF-8 片段丢弃。
pub fn compatible_list(node: &fdt::node::FdtNode<'_, '_>) -> Vec<String> {
    let mut list = Vec::new();
    let Some(raw) = node.property("compatible").map(|p| p.value) else {
        return list;
    };
    for item in raw.split(|b| *b == 0) {
        if item.is_empty() {
            continue;
        }
        if let Ok(text) = core::str::from_utf8(item) {
            list.push(String::from(text));
        }
    }
    list
}

/// 与 `virtio,mmio` 字符串精确一致才视为 virtio-mmio 节点。
pub fn is_virtio_mmio_compatible(compatibles: &[String]) -> bool {
    compatibles.iter().any(|c| c.as_str() == "virtio,mmio")
}
