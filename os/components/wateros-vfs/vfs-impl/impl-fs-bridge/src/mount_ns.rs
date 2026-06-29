//! per-task 挂载命名空间注册表（共享/复制语义对齐 cwd 表）。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::collections::BTreeMap;

use super::mount_table::MountNamespace;

/// 全局 per-task mount namespace 表。
// 本结构代码由AI完成
pub struct PerTaskMountNsRegistry {
    namespaces: BTreeMap<task::TaskId, MountNamespace>,
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
    pub fn namespace_for(&self, task_id: task::TaskId) -> Option<&MountNamespace> {
        let owner = self.effective_owner(task_id);
        self.namespaces.get(&owner)
    }

    /// 可变访问；惰性初始化后保证存在。
// 本方法代码由AI完成
    pub fn namespace_for_mut(&mut self, task_id: task::TaskId) -> &mut MountNamespace {
        self.init_task_mount_ns(task_id);
        let owner = self.effective_owner(task_id);
        self.namespaces
            .get_mut(&owner)
            .expect("mount namespace must exist after init")
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

    /// fork 时深拷贝父任务挂载表。
// 本方法代码由AI完成
    pub fn copy_mount_ns_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        let parent_ns = self
            .namespace_for(parent)
            .cloned()
            .unwrap_or_else(|| crate::mount_table::bootstrap_mount_namespace_snapshot());
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
        let shared = self
            .ref_counts
            .get(&owner)
            .copied()
            .unwrap_or(1) > 1
            || self
                .owners
                .iter()
                .any(|(tid, o)| *o == owner && *tid != task_id);
        if !shared {
            return;
        }
        let ns = self
            .namespaces
            .get(&owner)
            .cloned()
            .unwrap_or_default();
        self.owners.insert(task_id, task_id);
        self.ref_counts.insert(task_id, 1);
        self.namespaces.insert(task_id, ns);
    }
}
