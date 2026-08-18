//! SMP 生命周期类型。CPU ID 是平台选择的逻辑编号；当前 QEMU RISC-V profile
//! 直接使用 hart ID。
//!
//! 平台边界：[`PlatformSmp`] 只抽象离开当前 CPU 的操作（固件 HSM、IPI 传输和远端
//! fence）。清除本地中断 pending 位属于架构中断操作，刻意不放入此 trait。

use base::cpu::{CpuId, CpuMask};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformSmpError {
    /// 当前 profile 没有实现该能力；调用方必须停止对应 SMP 路径而非假装成功。
    Unsupported,
    /// CPU id 不在 machine 配置或 WaterOS 编译容量内。
    InvalidCpu,
    /// 固件已经启动该 hart；调用方仍须等待 OS online 确认后才能使用。
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

/// 随硬件 IPI 通知携带的软件层原因。SBI 和平台 IPI 寄存器只传递中断信号本身。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IpiKind {
    /// 目标 CPU 需要重新检查本地 runqueue；不推进全局 timer tick。
    Reschedule = 1 << 0,
    /// 目标 CPU 必须完成本地地址翻译缓存刷新。
    TlbShootdown = 1 << 1,
    /// 目标任务有必须在其 trap-return 安全点处理的状态变化（如 signal）。
    TaskNotify = 1 << 2,
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
    /// 同步失效所选 CPU 上的全部地址翻译；固件支持的平台无需目标 CPU 接收软件中断。
    fn flush_tlb_remote(mask: CpuMask) -> PlatformSmpResult<()>;
    /// 同步在所选 CPU 上执行指令缓存 fence；RISC-V 用它实现进程范围的 icache 刷新。
    fn flush_icache_remote(mask: CpuMask) -> PlatformSmpResult<()>;
    /// 初始化本 CPU 的 IPI 接收硬件。调用时机在 CPU-local/trap 基础设施就绪之后。
    fn init_ipi() -> PlatformSmpResult<()>;
}
