#![no_std]
//! 进程凭证（credentials）v0 契约：对齐 Linux `struct cred` 子集。
//!
//! impl-root 初始凭证为 root，并按 privileged 语义放行 set*id 更新；capabilities
//! 与 VFS 权限检查在此 trait 层预留。
//! `prctl(PR_CAPBSET_*)` 的权威语义最终将归口 `AccessCheck::has_cap`。

/// 任务在 cred 侧表中的索引（与 `task::TaskId` 数值一致，但不依赖 task crate）。
pub type TaskId = usize;

/// Linux 语义下的用户 ID（32-bit，与 stat/syscall 返回一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Uid(pub u32);

/// Linux 语义下的组 ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gid(pub u32);

/// 当前支持的 supplementary 组上限。
pub const SUPPLEMENTARY_GROUP_COUNT: usize = 32;

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
    pub supplementary_group_len: usize,
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
        supplementary_group_len: 1,
    };

    /// `getgroups(0)` 返回的 supplementary 组数量。
    #[inline]
    pub const fn supplementary_group_count(&self) -> isize {
        self.supplementary_group_len as isize
    }

    /// privileged `setgroups(2)` 语义：替换当前 supplementary 组列表。
    #[inline]
    pub fn set_supplementary_groups(&mut self, groups: &[Gid]) {
        self.supplementary_group_len = groups.len();
        let mut i = 0;
        while i < groups.len() {
            self.supplementary_groups[i] = groups[i];
            i += 1;
        }
    }

    /// privileged `setuid(2)` 语义：real/effective/saved/fs uid 全部更新。
    #[inline]
    pub fn set_uid(&mut self, uid: Uid) {
        self.real_uid = uid;
        self.effective_uid = uid;
        self.saved_uid = uid;
        self.fs_uid = uid;
    }

    /// privileged `setgid(2)` 语义：real/effective/saved/fs gid 全部更新。
    #[inline]
    pub fn set_gid(&mut self, gid: Gid) {
        self.real_gid = gid;
        self.effective_gid = gid;
        self.saved_gid = gid;
        self.fs_gid = gid;
    }

    /// privileged `setreuid(2)` 语义；`None` 表示 Linux 的 `-1` 保持不变。
    #[inline]
    pub fn set_reuid(&mut self, real_uid: Option<Uid>, effective_uid: Option<Uid>) {
        if let Some(uid) = real_uid {
            self.real_uid = uid;
        }
        if let Some(uid) = effective_uid {
            self.effective_uid = uid;
        }
        if real_uid.is_some() || effective_uid.is_some() {
            self.saved_uid = self.effective_uid;
            self.fs_uid = self.effective_uid;
        }
    }

    /// privileged `setregid(2)` 语义；`None` 表示 Linux 的 `-1` 保持不变。
    #[inline]
    pub fn set_regid(&mut self, real_gid: Option<Gid>, effective_gid: Option<Gid>) {
        if let Some(gid) = real_gid {
            self.real_gid = gid;
        }
        if let Some(gid) = effective_gid {
            self.effective_gid = gid;
        }
        if real_gid.is_some() || effective_gid.is_some() {
            self.saved_gid = self.effective_gid;
            self.fs_gid = self.effective_gid;
        }
    }

    /// privileged `setresuid(2)` 语义；`None` 表示 Linux 的 `-1` 保持不变。
    #[inline]
    pub fn set_resuid(
        &mut self,
        real_uid: Option<Uid>,
        effective_uid: Option<Uid>,
        saved_uid: Option<Uid>,
    ) {
        if let Some(uid) = real_uid {
            self.real_uid = uid;
        }
        if let Some(uid) = effective_uid {
            self.effective_uid = uid;
        }
        if let Some(uid) = saved_uid {
            self.saved_uid = uid;
        }
        self.fs_uid = self.effective_uid;
    }

    /// privileged `setresgid(2)` 语义；`None` 表示 Linux 的 `-1` 保持不变。
    #[inline]
    pub fn set_resgid(
        &mut self,
        real_gid: Option<Gid>,
        effective_gid: Option<Gid>,
        saved_gid: Option<Gid>,
    ) {
        if let Some(gid) = real_gid {
            self.real_gid = gid;
        }
        if let Some(gid) = effective_gid {
            self.effective_gid = gid;
        }
        if let Some(gid) = saved_gid {
            self.saved_gid = gid;
        }
        self.fs_gid = self.effective_gid;
    }
}

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
