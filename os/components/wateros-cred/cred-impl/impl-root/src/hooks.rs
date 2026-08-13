use api_v0::{AccessCheck, Capability, CredentialBackend, CredentialMutation, Gid,
             ProcessCredentials, TaskId, Uid};
use super::registry::registry;

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

pub fn try_credentials_for(tid: TaskId) -> Option<ProcessCredentials> {
    registry().exclusive_access().try_cred(tid)
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

pub fn set_supplementary_groups(tid: TaskId, groups: &[Gid]) {
    registry()
        .exclusive_access()
        .set_supplementary_groups(tid, groups);
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

pub fn may_chown(
    cred: &ProcessCredentials,
    inode_uid: Uid,
    inode_gid: Gid,
    new_uid: Option<u32>,
    new_gid: Option<u32>,
) -> bool {
    registry()
        .exclusive_access()
        .may_chown(cred, inode_uid, inode_gid, new_uid, new_gid)
}
