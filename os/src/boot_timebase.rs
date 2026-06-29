//! 引导期从 DTB 探测 CPU timebase 频率并注入 [`platform::time::set_frequency_hz`]。
//!
//! 须在 `driver::init_when_boot` 之后、首次 [`platform::timer`] 使用前调用；
//! DTB 指针与 driver 层 `read_fdt` 使用相同契约（常驻物理内存、合法 FDT 头）。

use fdt::Fdt;
use runtime::logging::warn;

/// DTB 探测结果来源（用于启动日志）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimebaseSource {
    Dtb,
    PlatformFallback,
}

/// 从 DTB 探测 timebase 并写入平台缓存；返回最终采用的 Hz。
pub fn probe_and_init_timebase(dtb_pa: usize) -> u64 {
    let (hz, source) = match probe_timebase_hz(dtb_pa) {
        Some(hz) => {
            if platform::time::set_frequency_hz(hz).is_err() {
                warn_invalid_and_fallback()
            } else {
                (hz, TimebaseSource::Dtb)
            }
        }
        None => warn_invalid_and_fallback(),
    };

    let source_label = match source {
        TimebaseSource::Dtb => "dtb",
        TimebaseSource::PlatformFallback => "platform-fallback",
    };
    warn!(
        "[boot] timebase-frequency={} Hz source={} dtb={:#x}",
        hz, source_label, dtb_pa
    );
    hz
}

/// DTB 无效或 `set_frequency_hz` 拒绝时，回退到平台默认频率并打标。
fn warn_invalid_and_fallback() -> (u64, TimebaseSource) {
    let hz = platform::time::frequency_hz().unwrap_or(0);
    (hz, TimebaseSource::PlatformFallback)
}

/// 从常驻物理地址解析 FDT，仅读 `/cpus` 树下的 `timebase-frequency`。
fn probe_timebase_hz(dtb_pa: usize) -> Option<u64> {
    if dtb_pa == 0 {
        return None;
    }
    let fdt = unsafe { Fdt::from_ptr(dtb_pa as *const u8) }.ok()?;
    read_cpus_timebase_hz(&fdt)
}

/// 仅读取 `/cpus` 或 `/cpus/cpu@*` 上的 `timebase-frequency`（不读外设 `clock-frequency`）。
fn read_cpus_timebase_hz(fdt: &Fdt<'_>) -> Option<u64> {
    let cpus = fdt.find_node("/cpus")?;
    if let Some(hz) = read_timebase_property(cpus) {
        return Some(hz);
    }
    for child in cpus.children() {
        if !child.name.starts_with("cpu") {
            continue;
        }
        if let Some(hz) = read_timebase_property(child) {
            return Some(hz);
        }
    }
    None
}

#[inline]
fn read_timebase_property(node: fdt::node::FdtNode<'_, '_>) -> Option<u64> {
    let raw = node.property("timebase-frequency")?.value;
    read_be_cell(raw)
}

/// 解析 FDT 大端 32/64 位 cell；其它长度视为无效。
#[inline]
fn read_be_cell(raw: &[u8]) -> Option<u64> {
    match raw.len() {
        4 => {
            let bytes = raw.get(0..4)?;
            Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64)
        }
        8 => {
            let bytes = raw.get(0..8)?;
            Some(u64::from_be_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6],
                bytes[7],
            ]))
        }
        _ => None,
    }
}
