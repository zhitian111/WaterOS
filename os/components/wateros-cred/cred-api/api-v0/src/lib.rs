#![no_std]
//! 进程凭证（credentials）v0 契约：对齐 Linux `struct cred` 子集。
//!
//! 首版 impl-root 恒为 root；capabilities 与 VFS 权限检查在此 trait 层预留。
//! `prctl(PR_CAPBSET_*)` 的权威语义最终将归口 `AccessCheck::has_cap`。

/// 任务在 cred 侧表中的索引（与 `task::TaskId` 数值一致，但不依赖 task crate）。
pub type TaskId = usize;

/// Linux 语义下的用户 ID（32-bit，与 stat/syscall 返回一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uid(pub u32);

/// Linux 语义下的组 ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gid(pub u32);

/// 首版 supplementary 组数量（G1：`getgroups` 返回 `[0]`）。
pub const SUPPLEMENTARY_GROUP_COUNT: usize = 1;

/// 进程凭证快照（八 ID + 固定长度 supplementary 组）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessCredentials {
    pub real_uid: Uid,
    pub real_gid: Gid,
    pub effective_uid: Uid,
    pub effective_gid: Gid,
    pub saved_uid: Uid,
    pub saved_gid: Gid,
    pub fs_uid: Uid,
    pub fs_gid: Gid,
    pub supplementary_groups: [Gid; SUPPLEMENTARY_GROUP_COUNT],
}

impl ProcessCredentials {
    /// bring-up 默认凭证：全部 ID 为 0，supplementary 组为 `[0]`。
    pub const ROOT: Self = Self {
        real_uid: Uid(0),
        real_gid: Gid(0),
        effective_uid: Uid(0),
        effective_gid: Gid(0),
        saved_uid: Uid(0),
        saved_gid: Gid(0),
        fs_uid: Uid(0),
        fs_gid: Gid(0),
        supplementary_groups: [Gid(0); SUPPLEMENTARY_GROUP_COUNT],
    };

    /// G1：`getgroups(0)` 返回的 supplementary 组数量。
    #[inline]
    pub const fn supplementary_group_count(&self) -> isize {
        SUPPLEMENTARY_GROUP_COUNT as isize
    }
}

/// 占位 capability 枚举；impl-root 恒返回 true。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// 占位项，后续按 Linux cap 编号扩展。
    Placeholder,
}

/// per-task 凭证生命周期后端。
pub trait CredentialBackend {
    /// 读取指定任务的凭证；无条目时由 impl 决定（impl-root：panic）。
    fn current(&self, tid: TaskId) -> ProcessCredentials;

    /// 新用户任务 spawn 后初始化。
    fn on_user_task_spawned(&mut self, tid: TaskId);

    /// fork 后复制父凭证到子任务。
    fn fork_cred(&mut self, parent: TaskId, child: TaskId);

    /// execve 后更新凭证（首版 no-op；将来解析 S_ISUID/S_ISGID）。
    fn on_exec(&mut self, tid: TaskId);

    /// 任务 reap 后删除侧表条目。
    fn drop_task_cred(&mut self, tid: TaskId);
}

/// 权限与 capability 检查（P1 占位）。
pub trait AccessCheck {
    /// 是否拥有指定 capability（impl-root：恒 true）。
    fn has_cap(&self, cred: &ProcessCredentials, cap: Capability) -> bool;

    /// 是否允许对 inode 元数据执行 access 类操作（impl-root：恒 true）。
    fn may_access_inode(
        &self,
        cred: &ProcessCredentials,
        inode_uid: Uid,
        inode_gid: Gid,
        mode: u32,
        access_mask: u32,
    ) -> bool;
}
