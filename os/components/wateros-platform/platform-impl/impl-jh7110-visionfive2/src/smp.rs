//! JH7110 SMP 后端：复用 OpenSBI HSM/IPI/remote fence，但排除 S7 监控核。
//!
//! 板级 DTB 中 `cpu@0`（`sifive,s7`）为 `status = "disabled"`，且 PLIC
//! `interrupts-extended` 只给它声明了 M 态上下文（irq 11），没有 S 态
//! 上下文（irq 9）；作为应用核启动会在 PLIC 上下文查找时失败。应用核为
//! U74 harts 1..=4（各带 M 态 + S 态上下文）。

use api_v0::smp::{HartStatus, PlatformSmp, PlatformSmpResult};
use base::cpu::{CpuId, CpuMask};
use opensbi_common::smp::OpenSbiSmp;

pub struct Jh7110Smp;

impl PlatformSmp for Jh7110Smp {
    fn start_cpu(cpu : CpuId, start_addr : usize, opaque : usize) -> PlatformSmpResult<()> {
        OpenSbiSmp::start_cpu(cpu, start_addr, opaque)
    }

    fn cpu_status(cpu : CpuId) -> PlatformSmpResult<HartStatus> {
        OpenSbiSmp::cpu_status(cpu)
    }

    fn configured_cpu_mask() -> CpuMask {
        // U74 应用核：hart 1..=4；hart 0（S7 监控核）不参与调度。
        CpuMask::from_bits(0b1_1110)
    }

    fn send_ipi(mask : CpuMask) -> PlatformSmpResult<()> {
        OpenSbiSmp::send_ipi(mask)
    }

    fn flush_tlb_remote(mask : CpuMask) -> PlatformSmpResult<()> {
        OpenSbiSmp::flush_tlb_remote(mask)
    }

    fn flush_icache_remote(mask : CpuMask) -> PlatformSmpResult<()> {
        OpenSbiSmp::flush_icache_remote(mask)
    }

    fn init_ipi() -> PlatformSmpResult<()> {
        OpenSbiSmp::init_ipi()
    }
}

pub use Jh7110Smp as SmpImpl;
