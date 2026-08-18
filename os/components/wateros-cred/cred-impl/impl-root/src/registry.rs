#![allow(static_mut_refs)]
//! impl-root：初始 root + privileged set*id 策略，以 `TaskId` 为 key 存储侧表（B2）。

extern crate alloc;

use alloc::collections::BTreeMap;
use api_v0::*;
use base::sync::MultiprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

/// 以 `TaskId` 为 key 的 per-task 凭证侧表。
pub struct PerTaskCredRegistry {
    /// 每个凭证所有者对应的快照；线程共享时只保留一份。
    creds: BTreeMap<TaskId, ProcessCredentials>,
    /// 任务到凭证所有者的映射；clone 线程可指向同一 owner。
    owners: BTreeMap<TaskId, TaskId>,
    /// owner 的引用数，归零时同步删除 `creds` 条目。
    ref_counts: BTreeMap<TaskId, usize>,
}

impl PerTaskCredRegistry {
    pub const fn new() -> Self {
        Self {
            creds: BTreeMap::new(),
            owners: BTreeMap::new(),
            ref_counts: BTreeMap::new(),
        }
    }

    fn ensure_owner(&mut self, tid: TaskId) -> TaskId {
        // 首次出现的 tid 成为自己的 owner；`or_insert` 保证重复调用不重置共享关系。
        self.owners.entry(tid).or_insert(tid);
        self.ref_counts.entry(tid).or_insert(1);
        self.effective_owner(tid)
    }

    fn effective_owner(&self, tid: TaskId) -> TaskId {
        self.owners
            .get(&tid)
            .copied()
            .unwrap_or(tid)
    }

    fn release_owner(&mut self, tid: TaskId) -> Option<TaskId> {
        let owner = self.owners.remove(&tid)?;
        if let Some(count) = self.ref_counts.get_mut(&owner) {
            // 使用饱和减法防止损坏的重复 reap 造成 usize 下溢；零计数随后移除。
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ref_counts.remove(&owner);
            }
        }
        Some(owner)
    }

    fn cred_or_panic(&self, tid: TaskId, context: &str) -> ProcessCredentials {
        let owner = self.effective_owner(tid);
        self.creds
            .get(&owner)
            .copied()
            .unwrap_or_else(|| panic!("[cred] no cred for tid={tid} ({context})"))
    }

    pub(super) fn try_cred(&self, tid: TaskId) -> Option<ProcessCredentials> {
        let owner = self.effective_owner(tid);
        self.creds.get(&owner).copied()
    }

    fn cred_mut_or_panic(&mut self, tid: TaskId, context: &str) -> &mut ProcessCredentials {
        let owner = self.effective_owner(tid);
        self.creds
            .get_mut(&owner)
            .unwrap_or_else(|| panic!("[cred] no cred for tid={tid} ({context})"))
    }

    pub(super) fn share_cred(&mut self, child: TaskId, parent: TaskId) {
        let _ = self.cred_or_panic(parent, "share_cred parent");
        if self.owners.contains_key(&child) {
            self.drop_task_cred(child);
        }
        let owner = self.effective_owner(parent);
        self.owners.insert(child, owner);
        let count = self.ref_counts.entry(owner).or_insert(0);
        *count = count.saturating_add(1);
    }
}

impl CredentialMutation for PerTaskCredRegistry {
    fn set_resuid(
        &mut self,
        tid: TaskId,
        real_uid: Option<Uid>,
        effective_uid: Option<Uid>,
        saved_uid: Option<Uid>,
    ) {
        self.cred_mut_or_panic(tid, "set_resuid")
            .set_resuid(real_uid, effective_uid, saved_uid);
    }

    fn set_resgid(
        &mut self,
        tid: TaskId,
        real_gid: Option<Gid>,
        effective_gid: Option<Gid>,
        saved_gid: Option<Gid>,
    ) {
        self.cred_mut_or_panic(tid, "set_resgid")
            .set_resgid(real_gid, effective_gid, saved_gid);
    }

    fn set_supplementary_groups(&mut self, tid: TaskId, groups: &[Gid]) {
        self.cred_mut_or_panic(tid, "set_supplementary_groups")
            .set_supplementary_groups(groups);
    }
}

impl CredentialBackend for PerTaskCredRegistry {
    fn current(&self, tid: TaskId) -> ProcessCredentials {
        self.cred_or_panic(tid, "current")
    }

    fn on_user_task_spawned(&mut self, tid: TaskId) {
        if self.owners.contains_key(&tid) {
            self.drop_task_cred(tid);
        }
        let owner = self.ensure_owner(tid);
        self.creds.insert(owner, ProcessCredentials::ROOT);
    }

    fn fork_cred(&mut self, parent: TaskId, child: TaskId) {
        let parent_cred = self.cred_or_panic(parent, "fork_cred parent");
        if self.owners.contains_key(&child) {
            self.drop_task_cred(child);
        }
        let owner = self.ensure_owner(child);
        self.creds.insert(owner, parent_cred);
    }

    fn on_exec(&mut self, _tid: TaskId) {
        // TODO(cred-exec-setuid): 解析可执行文件 S_ISUID/S_ISGID 并更新凭证。
    }

    fn drop_task_cred(&mut self, tid: TaskId) {
        // 缺少 owner 说明任务已被回收；幂等返回可处理重复清理路径。
        let Some(owner) = self.release_owner(tid) else {
            return;
        };
        if self.ref_counts.get(&owner).copied().unwrap_or(0) == 0 {
            self.creds.remove(&owner);
        }
        if tid != owner {
            self.creds.remove(&tid);
        }
    }
}

impl AccessCheck for PerTaskCredRegistry {
    fn has_cap(&self, cred: &ProcessCredentials, cap: Capability) -> bool {
        if cred.effective_uid.0 == 0 {
            return true;
        }
        let _ = cap;
        false
    }

    fn may_access_inode(
        &self,
        _cred: &ProcessCredentials,
        _inode_uid: Uid,
        _inode_gid: Gid,
        _mode: u32,
        _access_mask: u32,
    ) -> bool {
        true
    }

    fn may_chown(
        &self,
        cred: &ProcessCredentials,
        inode_uid: Uid,
        _inode_gid: Gid,
        new_uid: Option<u32>,
        new_gid: Option<u32>,
    ) -> bool {
        if cred.effective_uid.0 == 0 || self.has_cap(cred, Capability::Chown) {
            return true;
        }
        if cred.fs_uid != inode_uid {
            return false;
        }
        if let Some(uid) = new_uid {
            if uid != inode_uid.0 {
                return false;
            }
        }
        if let Some(gid) = new_gid {
            if gid == cred.effective_gid.0 {
                return true;
            }
            return cred
                .supplementary_groups
                .iter()
                .take(cred.supplementary_group_len)
                .any(|g| g.0 == gid);
        }
        true
    }
}

static mut CRED_REGISTRY: MaybeUninit<MultiprocessorSafeCell<PerTaskCredRegistry>> = MaybeUninit::uninit();
static CRED_REGISTRY_READY: AtomicUsize = AtomicUsize::new(0);

/// 返回全局凭证表；访问者必须持有返回的自旋锁，且初始化应发生在并发访问前。
pub(super) fn registry() -> &'static MultiprocessorSafeCell<PerTaskCredRegistry> {
    if CRED_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            CRED_REGISTRY.write(MultiprocessorSafeCell::new(PerTaskCredRegistry::new()));
        }
        CRED_REGISTRY_READY.store(1, Ordering::Release);
    }
    unsafe { &*CRED_REGISTRY.as_ptr() }
}
