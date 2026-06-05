#![no_std]
//! WaterOS 进程凭证聚合 crate：导出版本化 `api` 与 impl-root 门面。
//!
//! 生命周期 hook 由 syscall 路径与 bring-up call site 显式调用；`task` crate 不依赖本组件。

/// 版本化 cred 契约（v0）。
pub mod api {
    pub use ::api_v0::*;
}

#[cfg(feature = "impl-root")]
pub use impl_root as active_impl;

#[cfg(feature = "impl-root")]
pub use api_v0::{Gid, ProcessCredentials, TaskId, Uid};

#[cfg(feature = "impl-root")]
/// 新用户任务 spawn 后初始化 root 凭证。
pub fn on_user_task_spawned(tid: TaskId) {
    active_impl::on_user_task_spawned(tid);
}

#[cfg(feature = "impl-root")]
/// fork 后复制父任务凭证到子任务。
pub fn fork_cred(parent: TaskId, child: TaskId) {
    active_impl::fork_cred(parent, child);
}

#[cfg(feature = "impl-root")]
/// thread clone 后共享父任务凭证。
pub fn share_cred(parent: TaskId, child: TaskId) {
    active_impl::share_cred(parent, child);
}

#[cfg(feature = "impl-root")]
/// execve 后更新凭证（首版 no-op，保留 TODO(cred-exec-setuid) 扩展点）。
pub fn on_exec(tid: TaskId) {
    active_impl::on_exec(tid);
}

#[cfg(feature = "impl-root")]
/// 任务 reap 后删除侧表条目。
pub fn drop_task_cred(tid: TaskId) {
    active_impl::drop_task_cred(tid);
}

#[cfg(feature = "impl-root")]
/// 读取指定任务的凭证；无侧表条目时 panic（与 bring-up root 模型一致）。
pub fn credentials_for(tid: TaskId) -> ProcessCredentials {
    active_impl::current_credentials_for(tid)
}

#[cfg(feature = "impl-root")]
/// 读取当前运行任务的凭证；无当前任务或无侧表条目时 panic。
pub fn current_credentials() -> ProcessCredentials {
    let tid = task::current_task_id()
        .expect("[cred] current_credentials: no current task");
    active_impl::current_credentials_for(tid)
}
