//! Loongson 2K1000LA PM controller（PM1/PMU）复位后端。
//!
//! 控制器基址优先从 DTB（`loongson,ls2k1000-pmc` / `ls2k0500-pmc`）发现；PMON
//! 无 DTB 时回退到板级固定基址 0x1FE2_7000（BSP 事实）。

use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};
use core::sync::atomic::{AtomicUsize, Ordering};

const PM1_STS : usize = 0x0c;
const PM1_CNT : usize = 0x14;
const RST_CNT : usize = 0x30;
const RESET_VALUE : u32 = 1;
const POWEROFF_VALUE : u32 = 0x3c00;
/// PMON 无 DTB 时的板级固定 PM 控制器基址（BSP 事实）。
const FALLBACK_PM_BASE : usize = 0x1FE2_7000;

static PM_BASE : AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetDiscoveryError {
    MissingDtb,
    InvalidDtb,
    MissingController,
    InvalidRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegisterWrite {
    offset : usize,
    value : u32,
}

fn controller_node(node : fdt::node::FdtNode<'_, '_>) -> bool {
    node.property("compatible")
        .map(|property| {
            property.value
                    .split(|byte| *byte == 0)
                    .any(|item| {
                        item == b"loongson,ls2k1000-pmc" || item == b"loongson,ls2k0500-pmc"
                    })
        })
        .unwrap_or(false)
}

fn first_register(node : fdt::node::FdtNode<'_, '_>) -> Option<(usize, usize)> {
    let region = node.reg()?.next()?;
    Some((region.starting_address as usize, region.size?))
}

/// 从固件 DTB 发现 PM 控制器；不做任何 MMIO 访问。
pub fn discover_from_dtb(dtb_pa : usize) -> Result<usize, ResetDiscoveryError> {
    if dtb_pa == 0 {
        return Err(ResetDiscoveryError::MissingDtb);
    }
    let fdt = unsafe { fdt::Fdt::from_ptr(dtb_pa as *const u8) }
        .map_err(|_| ResetDiscoveryError::InvalidDtb)?;
    for node in fdt.all_nodes() {
        if !controller_node(node) {
            continue;
        }
        let (base, size) = first_register(node).ok_or(ResetDiscoveryError::InvalidRegion)?;
        if size <= RST_CNT {
            return Err(ResetDiscoveryError::InvalidRegion);
        }
        PM_BASE.store(base, Ordering::Release);
        return Ok(base);
    }
    Err(ResetDiscoveryError::MissingController)
}

/// 发现 PM 控制器：DTB 优先，PMON 无 DTB 时回退板级固定基址。
pub fn discover_pm_base(dtb_pa : usize) -> Result<usize, ResetDiscoveryError> {
    match discover_from_dtb(dtb_pa) {
        Ok(base) => Ok(base),
        Err(ResetDiscoveryError::MissingDtb | ResetDiscoveryError::MissingController) => {
            PM_BASE.store(FALLBACK_PM_BASE, Ordering::Release);
            Ok(FALLBACK_PM_BASE)
        }
        Err(error) => Err(error),
    }
}

pub fn discovered_pm_base() -> Option<usize> {
    match PM_BASE.load(Ordering::Acquire) {
        0 => None,
        base => Some(base),
    }
}

fn write_plan(reset_type : PlatformResetType) -> &'static [RegisterWrite] {
    match reset_type {
        // PM1_STS 先读后写（write-one-to-clear），值来自控制器状态而非常量。
        PlatformResetType::Shutdown => &[],
        PlatformResetType::ColdReboot | PlatformResetType::WarmReboot => &[
            RegisterWrite { offset : RST_CNT, value : RESET_VALUE },
        ],
    }
}

/// 执行 Linux 兼容的 2K1000 PM 序列（DTB 发现后）。
///
/// 硬件通常不会从这些写返回；返回 `Failed` 表示执行继续，板级契约仍需真机验证。
pub fn reset(reset_type : PlatformResetType,
             _reason : PlatformResetReason)
             -> PlatformResetResult<()> {
    let Some(base) = discovered_pm_base() else {
        return Err(PlatformResetError::Unsupported);
    };
    unsafe {
        if reset_type == PlatformResetType::Shutdown {
            let status = core::ptr::read_volatile((base + PM1_STS) as *const u32);
            core::ptr::write_volatile((base + PM1_STS) as *mut u32, status);
            core::ptr::write_volatile((base + PM1_CNT) as *mut u32, POWEROFF_VALUE);
        } else {
            for write in write_plan(reset_type) {
                core::ptr::write_volatile((base + write.offset) as *mut u32, write.value);
            }
        }
    }
    Err(PlatformResetError::Failed)
}

pub fn reboot(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reason)
}

pub fn shutdown(reason : PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dtb_uses_board_fixed_pm_base() {
        assert_eq!(discover_from_dtb(0), Err(ResetDiscoveryError::MissingDtb));
        assert_eq!(discover_pm_base(0), Ok(FALLBACK_PM_BASE));
        assert_eq!(discovered_pm_base(), Some(FALLBACK_PM_BASE));
    }

    #[test]
    fn plans_match_upstream_controller_contract() {
        assert_eq!(write_plan(PlatformResetType::ColdReboot),
                   &[RegisterWrite { offset : RST_CNT, value : RESET_VALUE }]);
        assert!(write_plan(PlatformResetType::Shutdown).is_empty());
    }
}
