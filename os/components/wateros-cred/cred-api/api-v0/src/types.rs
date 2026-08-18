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
    /// 创建进程时继承的真实用户 ID。
    pub real_uid: Uid,
    /// 创建进程时继承的真实组 ID。
    pub real_gid: Gid,
    /// 当前权限检查使用的有效用户 ID。
    pub effective_uid: Uid,
    /// 当前权限检查使用的有效组 ID。
    pub effective_gid: Gid,
    /// 可通过 set*id 恢复的用户 ID。
    pub saved_uid: Uid,
    /// 可通过 set*id 恢复的组 ID。
    pub saved_gid: Gid,
    /// 文件系统属主检查使用的用户 ID。
    pub fs_uid: Uid,
    /// 文件系统属组检查使用的组 ID。
    pub fs_gid: Gid,
    /// 固定容量数组；只有前 `supplementary_group_len` 项有效。
    pub supplementary_groups: [Gid; SUPPLEMENTARY_GROUP_COUNT],
    /// 有效 supplementary 组数量，范围必须为 `0..=SUPPLEMENTARY_GROUP_COUNT`。
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
    ///
    /// # Panics
    /// `groups.len()` 超过 [`SUPPLEMENTARY_GROUP_COUNT`] 时会因数组越界 panic；
    /// syscall 层必须在调用前返回 `EINVAL`，不能把用户长度直接传入。
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
