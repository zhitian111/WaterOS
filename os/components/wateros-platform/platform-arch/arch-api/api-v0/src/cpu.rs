//! 当前 CPU 标识的架构契约。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchCpuInitError {
    /// 启动入口提供的逻辑 CPU 超出容量，或与当前硬件 CPU 标识不一致。
    InvalidCpu,
}

/// 每 CPU arch 初始化的结果；成功不代表 scheduler 已将 CPU 标记为 online。
pub type ArchCpuInitResult<T> = core::result::Result<T, ArchCpuInitError>;
