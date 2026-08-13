//! OpenSBI HSM 与 IPI 后端。

use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpError, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};
use config::task::MAX_CPUS;

/// QEMU RISC-V `virt` 上通过 OpenSBI 执行的 SMP 运输后端。
///
/// PLATFORM_BOUNDARY: 这里只封装 HSM、IPI 和 remote fence SBI 调用；本地 `sip`
/// 清除、SSIE 开关以及 pending reason 都分别属于 arch 与聚合层。
pub struct QemuRiscv64OpenSbiSmp;

/// 将 SBI 返回值归一化为 WaterOS 的平台错误。
///
/// 保留无法识别的 raw code，避免新固件错误被错误地归类成 `Unsupported`。
fn result(ret: sbi::SbiRet) -> PlatformSmpResult<usize> {
    if ret.error == 0 {
        Ok(ret.value)
    } else {
        match ret.error as isize {
            -2 => Err(PlatformSmpError::Unsupported),
            -3 => Err(PlatformSmpError::InvalidCpu),
            -6 => Err(PlatformSmpError::AlreadyAvailable),
            _ => Err(PlatformSmpError::Firmware(ret.error)),
        }
    }
}

impl PlatformSmp for QemuRiscv64OpenSbiSmp {
    /// 调用 SBI HSM `hart_start`；成功仅表示固件接受请求。
    fn start_cpu(cpu: CpuId, start_addr: usize, opaque: usize) -> PlatformSmpResult<()> {
        if !cpu.fits_capacity(MAX_CPUS) {
            return Err(PlatformSmpError::InvalidCpu);
        }
        result(sbi::hart_start(
            cpu.raw(),
            start_addr,
            opaque,
        ))
        .map(|_| ())
    }

    /// 调用 HSM `hart_get_status`，供 BSP 启动诊断使用。
    fn cpu_status(cpu: CpuId) -> PlatformSmpResult<HartStatus> {
        if !cpu.fits_capacity(MAX_CPUS) {
            return Err(PlatformSmpError::InvalidCpu);
        }
        let value = result(sbi::hart_get_status(cpu.raw()))?;
        Ok(match value {
            0 => HartStatus::Started,
            1 => HartStatus::Stopped,
            2 => HartStatus::StartPending,
            3 => HartStatus::StopPending,
            other => HartStatus::Unknown(other),
        })
    }

    /// QEMU/WaterOS 当前编译容量对应的候选 hart 集合。
    fn configured_cpu_mask() -> CpuMask {
        CpuMask::from_bits((1u64 << MAX_CPUS) - 1)
    }

    /// 经 SBI IPI 扩展通知目标 hart；发送前的 reason 发布由聚合层完成。
    fn send_ipi(mask: CpuMask) -> PlatformSmpResult<()> {
        let hart_mask = sbi::HartMask::from_mask_base(mask.bits() as usize, 0);
        result(sbi::send_ipi(hart_mask)).map(|_| ())
    }

    /// 经 SBI remote fence 执行全范围 `sfence.vma`。
    fn flush_tlb_remote(mask: CpuMask) -> PlatformSmpResult<()> {
        let hart_mask = sbi::HartMask::from_mask_base(mask.bits() as usize, 0);
        result(sbi::remote_sfence_vma(
            hart_mask,
            0,
            usize::MAX,
        ))
        .map(|_| ())
    }

    /// 经 SBI RFENCE 扩展在目标 hart 上执行 `fence.i`。
    fn flush_icache_remote(mask: CpuMask) -> PlatformSmpResult<()> {
        let hart_mask = sbi::HartMask::from_mask_base(mask.bits() as usize, 0);
        result(sbi::remote_fence_i(hart_mask)).map(|_| ())
    }

    /// OpenSBI 不需要额外接收端控制；SSIE 由 arch boot 路径启用。
    fn init_ipi() -> PlatformSmpResult<()> {
        Ok(())
    }
}

pub use QemuRiscv64OpenSbiSmp as SmpImpl;
