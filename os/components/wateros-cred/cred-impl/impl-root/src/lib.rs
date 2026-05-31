#![no_std]
#![allow(static_mut_refs)]
//! impl-root：全员 root 凭证策略，按 `TaskId` 侧表存储（B2）。

extern crate alloc;

use alloc::vec::Vec;
use api_v0::{
    AccessCheck, Capability, CredentialBackend, Gid, ProcessCredentials, TaskId, Uid,
};
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

/// 按 `TaskId` 索引的 per-task 凭证侧表。
pub struct PerTaskCredRegistry {
    creds: Vec<Option<ProcessCredentials>>,
    owners: Vec<Option<TaskId>>,
    ref_counts: Vec<usize>,
}

impl PerTaskCredRegistry {
    pub const fn new() -> Self {
        Self {
            creds: Vec::new(),
            owners: Vec::new(),
            ref_counts: Vec::new(),
        }
    }

    fn slot_mut(&mut self, tid: TaskId) -> &mut Option<ProcessCredentials> {
        if self.creds.len() <= tid {
            self.creds.resize_with(tid + 1, || None);
            self.owners.resize_with(tid + 1, || None);
            self.ref_counts.resize(tid + 1, 0);
        }
        if self.owners[tid].is_none() {
            self.owners[tid] = Some(tid);
            self.ref_counts[tid] = 1;
        }
        let owner = self.effective_owner(tid);
        &mut self.creds[owner]
    }

    fn effective_owner(&self, tid: TaskId) -> TaskId {
        self.owners
            .get(tid)
            .and_then(|owner| *owner)
            .unwrap_or(tid)
    }

    fn release_owner(&mut self, tid: TaskId) -> Option<TaskId> {
        let owner = self.owners.get_mut(tid)?.take()?;
        if owner < self.ref_counts.len() && self.ref_counts[owner] > 0 {
            self.ref_counts[owner] -= 1;
        }
        Some(owner)
    }

    fn cred_or_panic(&self, tid: TaskId, context: &str) -> ProcessCredentials {
        let owner = self.effective_owner(tid);
        self.creds
            .get(owner)
            .and_then(|o| *o)
            .unwrap_or_else(|| panic!("[cred] no cred for tid={tid} ({context})"))
    }

    fn share_cred(&mut self, child: TaskId, parent: TaskId) {
        let _ = self.cred_or_panic(parent, "share_cred parent");
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task_cred(child);
        }
        if self.creds.len() <= child {
            self.creds.resize_with(child + 1, || None);
            self.owners.resize_with(child + 1, || None);
            self.ref_counts.resize(child + 1, 0);
        }
        let owner = self.effective_owner(parent);
        self.owners[child] = Some(owner);
        if owner >= self.ref_counts.len() {
            self.ref_counts.resize(owner + 1, 0);
        }
        self.ref_counts[owner] = self.ref_counts[owner].saturating_add(1);
    }
}

impl CredentialBackend for PerTaskCredRegistry {
    fn current(&self, tid: TaskId) -> ProcessCredentials {
        self.cred_or_panic(tid, "current")
    }

    fn on_user_task_spawned(&mut self, tid: TaskId) {
        *self.slot_mut(tid) = Some(ProcessCredentials::ROOT);
    }

    fn fork_cred(&mut self, parent: TaskId, child: TaskId) {
        let parent_cred = self.cred_or_panic(parent, "fork_cred parent");
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task_cred(child);
        }
        *self.slot_mut(child) = Some(parent_cred);
    }

    fn on_exec(&mut self, _tid: TaskId) {
        // TODO(cred-exec-setuid): 解析可执行文件 S_ISUID/S_ISGID 并更新凭证。
    }

    fn drop_task_cred(&mut self, tid: TaskId) {
        let Some(owner) = self.release_owner(tid) else {
            return;
        };
        if self.ref_counts.get(owner).copied().unwrap_or(0) == 0 {
            if let Some(slot) = self.creds.get_mut(owner) {
                *slot = None;
            }
        }
        if tid != owner && tid < self.creds.len() {
            self.creds[tid] = None;
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
