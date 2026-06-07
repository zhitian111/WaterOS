#![no_std]
#![allow(static_mut_refs)]
//! impl-root：初始 root + privileged set*id 策略，以 `TaskId` 为 key 存储侧表（B2）。

extern crate alloc;

use alloc::collections::BTreeMap;
use api_v0::{
    AccessCheck, Capability, CredentialBackend, CredentialMutation, Gid, ProcessCredentials, TaskId,
    Uid,
};
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

/// 以 `TaskId` 为 key 的 per-task 凭证侧表。
pub struct PerTaskCredRegistry {
    creds: BTreeMap<TaskId, ProcessCredentials>,
    owners: BTreeMap<TaskId, TaskId>,
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

    fn cred_mut_or_panic(&mut self, tid: TaskId, context: &str) -> &mut ProcessCredentials {
        let owner = self.effective_owner(tid);
        self.creds
            .get_mut(&owner)
            .unwrap_or_else(|| panic!("[cred] no cred for tid={tid} ({context})"))
    }

    fn share_cred(&mut self, child: TaskId, parent: TaskId) {
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
    fn has_cap(&self, _cred: &ProcessCredentials, _cap: Capability) -> bool {
        true
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
}

static mut CRED_REGISTRY: MaybeUninit<UniprocessorSafeCell<PerTaskCredRegistry>> = MaybeUninit::uninit();
static CRED_REGISTRY_READY: AtomicUsize = AtomicUsize::new(0);

fn registry() -> &'static UniprocessorSafeCell<PerTaskCredRegistry> {
    if CRED_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            CRED_REGISTRY.write(UniprocessorSafeCell::new(PerTaskCredRegistry::new()));
        }
        CRED_REGISTRY_READY.store(1, Ordering::Release);
    }
    unsafe { &*CRED_REGISTRY.as_ptr() }
}

pub fn on_user_task_spawned(tid: TaskId) {
    registry().exclusive_access().on_user_task_spawned(tid);
}

pub fn fork_cred(parent: TaskId, child: TaskId) {
    registry().exclusive_access().fork_cred(parent, child);
}

pub fn share_cred(parent: TaskId, child: TaskId) {
    registry().exclusive_access().share_cred(child, parent);
}

pub fn on_exec(tid: TaskId) {
    registry().exclusive_access().on_exec(tid);
}

pub fn drop_task_cred(tid: TaskId) {
    registry().exclusive_access().drop_task_cred(tid);
}

pub fn current_credentials_for(tid: TaskId) -> ProcessCredentials {
    registry().exclusive_access().current(tid)
}

pub fn set_resuid(
    tid: TaskId,
    real_uid: Option<Uid>,
    effective_uid: Option<Uid>,
    saved_uid: Option<Uid>,
) {
    registry()
        .exclusive_access()
        .set_resuid(tid, real_uid, effective_uid, saved_uid);
}

pub fn set_resgid(
    tid: TaskId,
    real_gid: Option<Gid>,
    effective_gid: Option<Gid>,
    saved_gid: Option<Gid>,
) {
    registry()
        .exclusive_access()
        .set_resgid(tid, real_gid, effective_gid, saved_gid);
}

pub fn has_cap(cred: &ProcessCredentials, cap: Capability) -> bool {
    registry().exclusive_access().has_cap(cred, cap)
}

pub fn may_access_inode(
    cred: &ProcessCredentials,
    inode_uid: Uid,
    inode_gid: Gid,
    mode: u32,
    access_mask: u32,
) -> bool {
    registry()
        .exclusive_access()
        .may_access_inode(cred, inode_uid, inode_gid, mode, access_mask)
}
