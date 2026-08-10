use api_v0::reset::{
    PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Loongson's upstream DT binding exposes the PM controller as a syscon node.
/// Keep the address DTB-derived: board firmware may relocate the controller.
static PM_BASE: AtomicUsize = AtomicUsize::new(0);
const PM1_STS: usize = 0x0c;
const PM1_CNT: usize = 0x14;
const RST_CNT: usize = 0x30;
const RESET_VALUE: u32 = 1;
const POWEROFF_VALUE: u32 = 0x3c00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetDiscoveryError {
    MissingDtb,
    InvalidDtb,
    MissingController,
    InvalidRegion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegisterWrite {
    offset: usize,
    value: u32,
}

fn controller_node(node: fdt::node::FdtNode<'_, '_>) -> bool {
    node.property("compatible")
        .map(|property| {
            property.value.split(|byte| *byte == 0).any(|item| {
                item == b"loongson,ls2k1000-pmc" || item == b"loongson,ls2k0500-pmc"
            })
        })
        .unwrap_or(false)
}

fn first_register(node: fdt::node::FdtNode<'_, '_>) -> Option<(usize, usize)> {
    let region = node.reg()?.next()?;
    Some((region.starting_address as usize, region.size?))
}

/// Discover the PM controller from firmware's DTB. No MMIO is performed here.
pub fn discover_from_dtb(dtb_pa: usize) -> Result<usize, ResetDiscoveryError> {
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

pub fn discovered_pm_base() -> Option<usize> {
    match PM_BASE.load(Ordering::Acquire) {
        0 => None,
        base => Some(base),
    }
}

fn write_plan(reset_type: PlatformResetType) -> &'static [RegisterWrite] {
    match reset_type {
        // PM1_STS is read first because its write-one-to-clear value is
        // controller state, not a constant.
        PlatformResetType::Shutdown => &[],
        PlatformResetType::ColdReboot | PlatformResetType::WarmReboot => &[
            RegisterWrite { offset: RST_CNT, value: RESET_VALUE },
        ],
    }
}

/// Execute the Linux-compatible 2K1000 PM sequence after DTB discovery.
///
/// Hardware normally does not return from these writes; `Failed` means that
/// execution continued and the board contract still needs validation there.
pub fn reset(reset_type: PlatformResetType, _reason: PlatformResetReason) -> PlatformResetResult<()> {
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

pub fn reboot(reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::ColdReboot, reason)
}

pub fn shutdown(reason: PlatformResetReason) -> PlatformResetResult<()> {
    reset(PlatformResetType::Shutdown, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dtb_discovery_keeps_backend_disabled() {
        assert_eq!(discover_from_dtb(0), Err(ResetDiscoveryError::MissingDtb));
        assert_eq!(reset(PlatformResetType::ColdReboot, PlatformResetReason::NoReason),
                   Err(PlatformResetError::Unsupported));
    }

    #[test]
    fn plans_match_upstream_controller_contract() {
        assert_eq!(write_plan(PlatformResetType::ColdReboot),
                   &[RegisterWrite { offset: RST_CNT, value: RESET_VALUE }]);
        assert!(write_plan(PlatformResetType::Shutdown).is_empty());
    }
}
