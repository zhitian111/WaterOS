//! DTB 访问与节点解析原语。
//!
//! 存储引导期传入的 DTB 物理指针，并提供只读解析助手；设备探测与注册等
//! transport 相关逻辑不在此处。

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{DriverError, DriverResult, IrqLine, MmioRegion};
use fdt::Fdt;

/// DTB 物理基址；为 0 时 [`read_fdt`] 返回 [`DriverError::NotFound`]。
static DTB_BASE_ADDR: AtomicUsize = AtomicUsize::new(0);

/// 保存引导期传入的 DTB 物理基址。
pub fn store(dtb_pa: usize) {
    DTB_BASE_ADDR.store(dtb_pa, Ordering::Release);
}

/// 当前保存的 DTB 物理基址（未保存时为 0）。
pub fn dtb_pa() -> usize {
    DTB_BASE_ADDR.load(Ordering::Acquire)
}

// `unsafe`：`dtb_pa` 指向的 DTB 在内核存活期内常驻且布局合法；返回的 `Fdt` 仅用于只读扫描。
pub fn read_fdt() -> DriverResult<Fdt<'static>> {
    let dtb = DTB_BASE_ADDR.load(Ordering::Acquire);
    if dtb == 0 {
        return Err(DriverError::NotFound);
    }
    let fdt = unsafe { Fdt::from_ptr(dtb as *const u8) }.map_err(|_| DriverError::InvalidDtb)?;
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
