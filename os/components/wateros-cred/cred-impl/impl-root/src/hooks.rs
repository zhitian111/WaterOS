use api_v0::{AccessCheck, Capability, CredentialBackend, CredentialMutation, Gid,
             ProcessCredentials, TaskId, Uid};
use super::registry::registry;

/// 为新用户任务建立独立的 root 凭证条目；重复 tid 会先清理旧条目。
pub fn on_user_task_spawned(tid: TaskId) {
    registry().exclusive_access().on_user_task_spawned(tid);
}

/// fork 后复制父任务快照，父子随后可独立修改凭证。
pub fn fork_cred(parent: TaskId, child: TaskId) {
    registry().exclusive_access().fork_cred(parent, child);
}

/// thread clone 后让 child 与 parent 共享同一个凭证所有者和引用计数。
pub fn share_cred(parent: TaskId, child: TaskId) {
    registry().exclusive_access().share_cred(child, parent);
}

/// execve 生命周期钩子；当前 root 实现不改变凭证，保留后端扩展点。
pub fn on_exec(tid: TaskId) {
    registry().exclusive_access().on_exec(tid);
}

/// reap 时释放任务对凭证所有者的引用；最后一个引用消失后删除快照。
pub fn drop_task_cred(tid: TaskId) {
    registry().exclusive_access().drop_task_cred(tid);
}

/// 读取任务凭证快照；tid 无条目时按 root bring-up 契约 panic。
pub fn current_credentials_for(tid: TaskId) -> ProcessCredentials {
    registry().exclusive_access().current(tid)
}

/// 尝试读取任务凭证；任务尚未发布或已回收时返回 `None`。
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

/// 运行凭证侧表的内核态可用性自检；测试使用局部注册表，不污染全局任务状态。
#[cfg(feature = "self_test")]
pub fn self_test() {
    use api_v0::{CredentialBackend, CredentialMutation};

    log::info!("[cred] self_test begin");
    let mut registry = super::registry::PerTaskCredRegistry::new();
    registry.on_user_task_spawned(1);
    assert_eq!(registry.current(1), ProcessCredentials::ROOT);

    registry.fork_cred(1, 2);
    assert_eq!(registry.current(2), ProcessCredentials::ROOT);
    registry.set_resuid(2, Some(Uid(1000)), Some(Uid(1000)), Some(Uid(1000)));
    assert_eq!(registry.current(1), ProcessCredentials::ROOT);
    assert_eq!(registry.current(2).effective_uid, Uid(1000));

    registry.set_supplementary_groups(2, &[Gid(1000), Gid(1001)]);
    assert_eq!(registry.current(2).supplementary_group_count(), 2);
    registry.drop_task_cred(2);
    registry.drop_task_cred(1);
    assert!(registry.try_cred(1).is_none());
    assert!(registry.try_cred(2).is_none());
    log::info!("[cred] self_test complete; temporary credentials reclaimed");
}
