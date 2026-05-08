//! 内核任务首次运行时的 **不透明启动载荷**：由 arch 任务入口跳板传入，再调用实际 `KernelTaskEntry`。
//!
//! 留在实现层，避免在公共 `task_api` 中暴露具体启动协议。

use api_v0::KernelTaskEntry;

/// 由 arch 任务入口跳板交给任务运行时的不透明启动数据；公共任务 API 无需暴露启动协议细节。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct TaskBootstrap {
    pub entry : KernelTaskEntry,
    pub arg : usize,
}

impl TaskBootstrap {
    /// 构造启动载荷：`entry` 为实际内核任务体，`arg` 透传给该入口。
    #[inline]
    pub const fn new(entry : KernelTaskEntry, arg : usize) -> Self { Self { entry, arg } }

    /// 跳转到内核任务入口；仅在首次被调度到该任务时由 arch 跳板调用一次。
    #[inline]
    pub fn run(&self) -> ! { (self.entry)(self.arg) }
}
