//! Platform SMP lifecycle types. CPU ids are logical ids selected by the
//! platform; the QEMU RISC-V profile currently uses the hart id directly.
//!
//! PLATFORM_BOUNDARY: [`PlatformSmp`] only abstracts operations that leave the
//! current CPU (firmware HSM, IPI transport, remote fence). Clearing a local
//! interrupt pending bit is an arch interrupt operation and deliberately is
//! not part of this trait.

use base::cpu::{CpuId, CpuMask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformSmpError {
    /// 当前 profile 没有实现该能力；调用方必须停止对应 SMP 路径而非假装成功。
    Unsupported,
    /// CPU id 不在 machine 配置或 WaterOS 编译容量内。
    InvalidCpu,
    /// The hart has already been started by firmware.  Callers should still
    /// wait for the OS-level online acknowledgement before using it.
    AlreadyAvailable,
    /// 固件/控制器返回的原始错误码，保留数值便于与 SBI/手册对照。
    Firmware(usize),
}

/// 平台 SMP 操作的统一结果类型。
pub type PlatformSmpResult<T> = core::result::Result<T, PlatformSmpError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HartStatus {
    /// 固件认为 hart 已开始执行。
    Started,
    /// hart 处于可启动状态。
    Stopped,
    /// firmware 已接收启动请求，但 OS 入口尚未确认 online。
    StartPending,
    /// firmware 正在停止 hart。
    StopPending,
    /// profile 无法映射的原始状态值。
    Unknown(usize),
}

/// Software-level reason carried alongside the hardware IPI notification.
/// SBI and platform IPI registers only deliver the interrupt signal itself.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiKind {
    /// 目标 CPU 需要重新检查本地 runqueue；不推进全局 timer tick。
    Reschedule = 1 << 0,
    /// 目标 CPU 必须完成本地地址翻译缓存刷新。
    TlbShootdown = 1 << 1,
    /// 目标任务有必须在其 trap-return 安全点处理的状态变化（如 signal）。
    TaskNotify = 1 << 2,
    /// 目标 CPU 应把 slab magazine 归还中央表并确认内存压力回收。
    AllocatorDrain = 1 << 3,
}

impl IpiKind {
    /// 取得可存入 pending IPI 位图的单 bit 编码。
    #[inline]
    pub const fn bits(self) -> u8 {
        self as u8
    }
}

pub trait PlatformSmp {
    /// 通过固件或板级 IPI 控制器启动辅助 CPU。
    fn start_cpu(cpu: CpuId, start_addr: usize, opaque: usize) -> PlatformSmpResult<()>;
    /// 查询 firmare/控制器的 hart 状态；该状态不等价于 OS online。
    fn cpu_status(cpu: CpuId) -> PlatformSmpResult<HartStatus>;
    /// 返回 machine 配置容量，不筛选已 online 的 CPU。
    fn configured_cpu_mask() -> CpuMask;
    /// 向目标 CPU 发送 IPI 的运输层；不负责目标 CPU 的本地 pending 位清除。
    fn send_ipi(mask: CpuMask) -> PlatformSmpResult<()>;
    /// Synchronously invalidate all address translations on the selected CPUs.
    ///
    /// Firmware-backed platforms may complete this without requiring the
    /// target CPUs to take a supervisor software interrupt.
    fn flush_tlb_remote(mask: CpuMask) -> PlatformSmpResult<()>;
    /// 初始化本 CPU 的 IPI 接收硬件。调用时机在 CPU-local/trap 基础设施就绪之后。
    fn init_ipi() -> PlatformSmpResult<()>;
}
