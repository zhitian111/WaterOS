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
}

impl PerTaskCredRegistry {
    pub const fn new() -> Self {
        Self { creds: Vec::new() }
    }

    fn slot_mut(&mut self, tid: TaskId) -> &mut Option<ProcessCredentials> {
        if self.creds.len() <= tid {
            self.creds.resize_with(tid + 1, || None);
        }
        &mut self.creds[tid]
    }

    fn cred_or_panic(&self, tid: TaskId, context: &str) -> ProcessCredentials {
        self.creds
            .get(tid)
            .and_then(|o| *o)
            .unwrap_or_else(|| panic!("[cred] no cred for tid={tid} ({context})"))
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
        *self.slot_mut(child) = Some(parent_cred);
    }

    fn on_exec(&mut self, _tid: TaskId) {
        // TODO(cred-exec-setuid): 解析可执行文件 S_ISUID/S_ISGID 并更新凭证。
    }

    fn drop_task_cred(&mut self, tid: TaskId) {
        if let Some(slot) = self.creds.get_mut(tid) {
            *slot = None;
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
