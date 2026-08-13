use super::*;

/// Linux capability 子集；与 `capget(2)` 返回的 effective/permitted 位对齐。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    /// `CAP_CHOWN`：任意修改文件 uid/gid。
    Chown,
    /// `CAP_SYS_ADMIN`：trusted/security xattr 等管理操作。
    SysAdmin,
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

/// per-task 凭证 ID 更新后端。
pub trait CredentialMutation {
    /// 更新 uid 三元组；`None` 表示 Linux set*id API 的 `-1` 保持不变。
    fn set_resuid(
        &mut self,
        tid: TaskId,
        real_uid: Option<Uid>,
        effective_uid: Option<Uid>,
        saved_uid: Option<Uid>,
    );

    /// 更新 gid 三元组；`None` 表示 Linux set*id API 的 `-1` 保持不变。
    fn set_resgid(
        &mut self,
        tid: TaskId,
        real_gid: Option<Gid>,
        effective_gid: Option<Gid>,
        saved_gid: Option<Gid>,
    );

    /// 替换 supplementary 组列表。
    fn set_supplementary_groups(&mut self, tid: TaskId, groups: &[Gid]);
}

/// 权限与 capability 检查（P1 占位）。
pub trait AccessCheck {
    /// 是否拥有指定 capability（impl-root 当前恒 true）。
    fn has_cap(&self, cred: &ProcessCredentials, cap: Capability) -> bool;

    /// 是否允许对 inode 元数据执行 access 类操作（impl-root 当前恒 true）。
    fn may_access_inode(
        &self,
        cred: &ProcessCredentials,
        inode_uid: Uid,
        inode_gid: Gid,
        mode: u32,
        access_mask: u32,
    ) -> bool;

    /// 是否允许 `chown(2)` / `fchownat(2)` 将 inode 属主/属组改为 `new_uid`/`new_gid`。
    /// `None` 表示对应字段不修改（Linux `-1`）。
    fn may_chown(
        &self,
        cred: &ProcessCredentials,
        inode_uid: Uid,
        inode_gid: Gid,
        new_uid: Option<u32>,
        new_gid: Option<u32>,
    ) -> bool;
}
