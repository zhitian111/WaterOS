//! 当前 CPU 标识的架构契约。

use base::cpu::CpuId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchCpuInitError {
    InvalidCpu,
}

pub type ArchCpuInitResult<T> = core::result::Result<T, ArchCpuInitError>;

pub trait ArchCpu {
    /// 返回当前执行 Rust 内核代码的逻辑 CPU。
    fn current_cpu_id() -> CpuId;

    /// 在当前 CPU 进入普通内核路径前建立架构 CPU-local 状态。
    fn init_current_cpu(cpu : CpuId) -> ArchCpuInitResult<()>;
}
