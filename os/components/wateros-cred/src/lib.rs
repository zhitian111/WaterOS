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
pub use api_v0::{Capability, Gid, ProcessCredentials, TaskId, Uid};

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

#[cfg(feature = "impl-root")]
/// 设置当前任务 uid；impl-root 阶段按 privileged `setuid(2)` 更新所有 uid。
pub fn set_uid(uid: Uid) {
    let tid = current_tid_for_mutation("set_uid");
    active_impl::set_resuid(tid, Some(uid), Some(uid), Some(uid));
}

#[cfg(feature = "impl-root")]
/// 设置当前任务 gid；impl-root 阶段按 privileged `setgid(2)` 更新所有 gid。
pub fn set_gid(gid: Gid) {
    let tid = current_tid_for_mutation("set_gid");
    active_impl::set_resgid(tid, Some(gid), Some(gid), Some(gid));
}

#[cfg(feature = "impl-root")]
/// 设置当前任务 real/effective uid；`None` 表示 Linux `-1` 保持不变。
pub fn set_reuid(real_uid: Option<Uid>, effective_uid: Option<Uid>) {
    let tid = current_tid_for_mutation("set_reuid");
    let saved_uid = if real_uid.is_some() || effective_uid.is_some() {
        let current = active_impl::current_credentials_for(tid);
        Some(effective_uid.unwrap_or(current.effective_uid))
    } else {
        None
    };
    active_impl::set_resuid(tid, real_uid, effective_uid, saved_uid);
}

#[cfg(feature = "impl-root")]
/// 设置当前任务 real/effective gid；`None` 表示 Linux `-1` 保持不变。
pub fn set_regid(real_gid: Option<Gid>, effective_gid: Option<Gid>) {
    let tid = current_tid_for_mutation("set_regid");
    let saved_gid = if real_gid.is_some() || effective_gid.is_some() {
        let current = active_impl::current_credentials_for(tid);
        Some(effective_gid.unwrap_or(current.effective_gid))
    } else {
        None
    };
    active_impl::set_resgid(tid, real_gid, effective_gid, saved_gid);
}

#[cfg(feature = "impl-root")]
/// 设置当前任务 real/effective/saved uid；`None` 表示 Linux `-1` 保持不变。
pub fn set_resuid(real_uid: Option<Uid>, effective_uid: Option<Uid>, saved_uid: Option<Uid>) {
    let tid = current_tid_for_mutation("set_resuid");
    active_impl::set_resuid(tid, real_uid, effective_uid, saved_uid);
}

#[cfg(feature = "impl-root")]
/// 设置当前任务 real/effective/saved gid；`None` 表示 Linux `-1` 保持不变。
pub fn set_resgid(real_gid: Option<Gid>, effective_gid: Option<Gid>, saved_gid: Option<Gid>) {
    let tid = current_tid_for_mutation("set_resgid");
    active_impl::set_resgid(tid, real_gid, effective_gid, saved_gid);
}

#[cfg(feature = "impl-root")]
fn current_tid_for_mutation(context: &str) -> TaskId {
    task::current_task_id().unwrap_or_else(|| panic!("[cred] {context}: no current task"))
}

#[cfg(feature = "impl-root")]
/// 查询 capability；impl-root 阶段恒为 true。
pub fn has_cap(cred: &ProcessCredentials, cap: Capability) -> bool {
    active_impl::has_cap(cred, cap)
}

#[cfg(feature = "impl-root")]
/// 查询 inode 访问权限；impl-root 阶段恒为 true。
pub fn may_access_inode(
    cred: &ProcessCredentials,
    inode_uid: Uid,
    inode_gid: Gid,
    mode: u32,
    access_mask: u32,
) -> bool {
    active_impl::may_access_inode(cred, inode_uid, inode_gid, mode, access_mask)
}
