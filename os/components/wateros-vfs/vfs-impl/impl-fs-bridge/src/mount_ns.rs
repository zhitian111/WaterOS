//! per-task 挂载命名空间注册表（共享/复制语义对齐 cwd 表）。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;

use super::mount_table::MountNamespace;

/// 全局 per-task mount namespace 表。
// 本结构代码由AI完成
pub struct PerTaskMountNsRegistry {
    namespaces: BTreeMap<task::TaskId, Arc<MountNamespace>>,
    owners: BTreeMap<task::TaskId, task::TaskId>,
    ref_counts: BTreeMap<task::TaskId, usize>,
}

impl PerTaskMountNsRegistry {
    pub const fn new() -> Self {
        Self {
            namespaces: BTreeMap::new(),
            owners: BTreeMap::new(),
            ref_counts: BTreeMap::new(),
        }
    }

// 本方法代码由AI完成
    fn ensure_owner(&mut self, task_id: task::TaskId) -> task::TaskId {
        self.owners.entry(task_id).or_insert(task_id);
        self.ref_counts.entry(task_id).or_insert(1);
        self.effective_owner(task_id)
    }

    fn effective_owner(&self, task_id: task::TaskId) -> task::TaskId {
        self.owners.get(&task_id).copied().unwrap_or(task_id)
    }

// 本方法代码由AI完成
    fn release_owner(&mut self, task_id: task::TaskId) -> Option<task::TaskId> {
        let owner = self.owners.remove(&task_id)?;
        if let Some(count) = self.ref_counts.get_mut(&owner) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ref_counts.remove(&owner);
            }
        }
        Some(owner)
    }

    /// 初始化任务挂载命名空间（继承 bootstrap 快照）。
// 本方法代码由AI完成
    pub fn init_task_mount_ns(&mut self, task_id: task::TaskId) {
        let owner = self.ensure_owner(task_id);
        self.namespaces.entry(owner).or_insert_with(|| {
            crate::mount_table::bootstrap_mount_namespace_snapshot()
        });
    }

    /// 只读访问任务挂载命名空间；未初始化时 `None`。
// 本方法代码由AI完成
    pub fn namespace_for(&self, task_id: task::TaskId) -> Option<&Arc<MountNamespace>> {
        let owner = self.effective_owner(task_id);
        self.namespaces.get(&owner)
    }

    /// 可变访问；惰性初始化后保证存在。
// 本方法代码由AI完成
    pub fn namespace_for_mut(&mut self, task_id: task::TaskId) -> &mut MountNamespace {
        self.init_task_mount_ns(task_id);
        let owner = self.effective_owner(task_id);
        Arc::make_mut(self.namespaces
                          .get_mut(&owner)
                          .expect("mount namespace must exist after init"))
    }

// 本方法代码由AI完成
    pub fn drop_task(&mut self, task_id: task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts.get(&owner).copied().unwrap_or(0) == 0 {
            self.namespaces.remove(&owner);
        }
        if task_id != owner {
            self.namespaces.remove(&task_id);
        }
    }

    /// fork/`CLONE_NEWNS` 时共享只读快照，首次修改再 COW。
// 本方法代码由AI完成
    pub fn copy_mount_ns_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        let parent_ns = self.namespace_for(parent)
                            .cloned()
                            .unwrap_or_else(crate::mount_table::bootstrap_mount_namespace_snapshot);
        if self.owners.contains_key(&child) {
            self.drop_task(child);
        }
        let child_owner = self.ensure_owner(child);
        self.namespaces.insert(child_owner, parent_ns);
    }

// 本方法代码由AI完成
    pub fn share_mount_ns_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.init_task_mount_ns(parent);
        if self.owners.contains_key(&child) {
            self.drop_task(child);
        }
        let owner = self.effective_owner(parent);
        self.owners.insert(child, owner);
        let count = self.ref_counts.entry(owner).or_insert(0);
        *count = count.saturating_add(1);
    }

    /// `unshare(CLONE_NEWNS)`：若与父/兄弟共享命名空间则复制一份。
// 本方法代码由AI完成
    pub fn unshare_mount_ns(&mut self, task_id: task::TaskId) {
        let owner = self.effective_owner(task_id);
        let count = self.ref_counts.get(&owner).copied().unwrap_or(1);
        if count <= 1 {
            return;
        }
        let ns = self.namespaces.get(&owner).cloned().unwrap_or_default();

        if task_id != owner {
            self.owners.insert(task_id, task_id);
            self.ref_counts.insert(owner, count - 1);
            self.ref_counts.insert(task_id, 1);
            self.namespaces.insert(task_id, ns);
            return;
        }

        // The owner cannot leave its own slot while the remaining tasks still point at it.
        // Re-home those tasks under one of their members, then retain the old slot for task_id.
        let new_owner = self.owners
                            .iter()
                            .find_map(|(tid, current_owner)| {
                                (*tid != task_id && *current_owner == owner).then_some(*tid)
                            })
                            .expect("shared mount namespace must have another member");
        for (tid, current_owner) in self.owners.iter_mut() {
            if *tid != task_id && *current_owner == owner {
                *current_owner = new_owner;
            }
        }
        self.namespaces.insert(new_owner, ns.clone());
        self.ref_counts.insert(new_owner, count - 1);
        self.ref_counts.insert(task_id, 1);
        self.namespaces.insert(task_id, ns);
    }
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use super::PerTaskMountNsRegistry;

    #[test]
    fn copied_namespace_is_lazy_cow() {
        let mut registry = PerTaskMountNsRegistry::new();
        registry.init_task_mount_ns(1);
        registry.copy_mount_ns_from_parent(2, 1);
        assert!(Arc::ptr_eq(registry.namespace_for(1).unwrap(),
                            registry.namespace_for(2).unwrap()));

        let _ = registry.namespace_for_mut(2);
        assert!(!Arc::ptr_eq(registry.namespace_for(1).unwrap(),
                             registry.namespace_for(2).unwrap()));
    }

    #[test]
    fn shared_namespace_keeps_one_owner_slot() {
        let mut registry = PerTaskMountNsRegistry::new();
        registry.init_task_mount_ns(1);
        registry.share_mount_ns_from_parent(2, 1);
        let _ = registry.namespace_for_mut(2);
        assert!(Arc::ptr_eq(registry.namespace_for(1).unwrap(),
                            registry.namespace_for(2).unwrap()));
    }

    #[test]
    fn unshare_detaches_member_and_owner() {
        let mut registry = PerTaskMountNsRegistry::new();
        registry.init_task_mount_ns(1);
        registry.share_mount_ns_from_parent(2, 1);
        registry.share_mount_ns_from_parent(3, 1);

        registry.unshare_mount_ns(2);
        let _ = registry.namespace_for_mut(2);
        assert!(!Arc::ptr_eq(registry.namespace_for(1).unwrap(),
                             registry.namespace_for(2).unwrap()));
        assert!(Arc::ptr_eq(registry.namespace_for(1).unwrap(),
                            registry.namespace_for(3).unwrap()));

        registry.unshare_mount_ns(1);
        let _ = registry.namespace_for_mut(1);
        assert!(!Arc::ptr_eq(registry.namespace_for(1).unwrap(),
                             registry.namespace_for(3).unwrap()));
        assert_eq!(registry.effective_owner(3), 3);
    }

    #[test]
    fn dropping_last_shared_member_reclaims_owner_slot() {
        let mut registry = PerTaskMountNsRegistry::new();
        registry.init_task_mount_ns(1);
        registry.share_mount_ns_from_parent(2, 1);
        registry.drop_task(1);
        assert!(registry.namespace_for(2).is_some());
        registry.drop_task(2);
        assert!(registry.namespaces.is_empty());
        assert!(registry.ref_counts.is_empty());
    }
}
