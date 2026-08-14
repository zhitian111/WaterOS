//! JH7110 MMC 资源描述与 fail-closed 激活门控（任务 06 范围）。
//!
//! 任务 06 只迁移类型与 `bring_up_plan`（不触达硬件）；DW MMC 控制器 PIO 与
//! SD 协议实现（`impl-dw-mmc`）在任务 07 接入，届时本模块补齐 probe/register 路径。

use alloc::vec::Vec;
use api_v0::MmioRegion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcActivationBlocker {
    InvalidMmio,
    InvalidIrq,
    InvalidBusWidth,
    MissingBiuClock,
    MissingCiuClock,
    MissingReset,
    MissingSysreg,
    MissingTargetFrequency,
    MissingFifoDepth,
    InvalidTargetFrequency,
    InvalidFifoDepth,
    HardwareEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmcControllerConfig {
    pub target_frequency_hz : u32,
    pub fifo_depth : u32,
    pub bus_width : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcConfigError {
    InvalidStaticResources,
    MissingTargetFrequency,
    MissingFifoDepth,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MmcHardwareEvidence {
    pub clock_verified : bool,
    pub reset_verified : bool,
    pub irq_verified : bool,
    pub card_path_verified : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcBringUpPlan {
    pub host : MmcHostDescription,
    pub blockers : Vec<MmcActivationBlocker>,
}

impl MmcBringUpPlan {
    /// 硬件激活保持不可用，直到板级时钟/reset/pinmux/卡与控制器行为得到验证。
    pub const fn can_activate(&self) -> bool {
        false
    }

    /// 不访问任何寄存器，仅按外部证据评估是否可解除静态 `HardwareEvidence` 门控。
    pub fn activation_ready(&self, evidence : MmcHardwareEvidence) -> bool {
        self.blockers.len() == 1 &&
        self.blockers[0] == MmcActivationBlocker::HardwareEvidence &&
        evidence.clock_verified && evidence.reset_verified && evidence.irq_verified &&
        evidence.card_path_verified
    }

    /// 只产生协议/控制器参数；不触碰时钟、reset、pinmux、电源或 MMIO。
    pub fn controller_config(&self) -> Result<MmcControllerConfig, MmcConfigError> {
        if self.blockers.iter().any(|blocker| {
            !matches!(blocker, MmcActivationBlocker::HardwareEvidence)
        }) {
            return Err(MmcConfigError::InvalidStaticResources);
        }
        let target_frequency_hz =
            self.host.max_frequency_hz.ok_or(MmcConfigError::MissingTargetFrequency)?;
        let fifo_depth = self.host.fifo_depth.ok_or(MmcConfigError::MissingFifoDepth)?;
        Ok(MmcControllerConfig { target_frequency_hz,
                                  fifo_depth,
                                  bus_width : self.host.bus_width })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecifier {
    pub provider : u32,
    pub args : Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysregField {
    pub provider : u32,
    pub offset : u32,
    pub shift : u8,
    pub mask : u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcHostDescription {
    pub mmio : MmioRegion,
    pub irq : u32,
    pub bus_width : u8,
    pub max_frequency_hz : Option<u32>,
    pub fifo_depth : Option<u32>,
    pub non_removable : bool,
    pub biu_clock : ResourceSpecifier,
    pub ciu_clock : ResourceSpecifier,
    pub reset : ResourceSpecifier,
    pub sysreg : Option<SysregField>,
}

pub fn bring_up_plan(host : &MmcHostDescription) -> MmcBringUpPlan {
    let mut blockers = Vec::new();
    if host.mmio.base == 0 || host.mmio.base % 4 != 0 || host.mmio.size < 0x100 {
        blockers.push(MmcActivationBlocker::InvalidMmio);
    }
    if host.irq == 0 {
        blockers.push(MmcActivationBlocker::InvalidIrq);
    }
    if !matches!(host.bus_width, 1 | 4 | 8) {
        blockers.push(MmcActivationBlocker::InvalidBusWidth);
    }
    if host.biu_clock.provider == 0 || host.biu_clock.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingBiuClock);
    }
    if host.ciu_clock.provider == 0 || host.ciu_clock.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingCiuClock);
    }
    if host.reset.provider == 0 || host.reset.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingReset);
    }
    if host.sysreg.is_none() {
        blockers.push(MmcActivationBlocker::MissingSysreg);
    }
    if host.max_frequency_hz.is_none() {
        blockers.push(MmcActivationBlocker::MissingTargetFrequency);
    } else if host.max_frequency_hz == Some(0) {
        blockers.push(MmcActivationBlocker::InvalidTargetFrequency);
    }
    if host.fifo_depth.is_none() {
        blockers.push(MmcActivationBlocker::MissingFifoDepth);
    } else if !host.fifo_depth.is_some_and(|depth| (2..=4096).contains(&depth)) {
        blockers.push(MmcActivationBlocker::InvalidFifoDepth);
    }
    blockers.push(MmcActivationBlocker::HardwareEvidence);
    MmcBringUpPlan { host : host.clone(), blockers }
}
