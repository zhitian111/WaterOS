//! 按 [`task::TaskId`] 索引的 per-task 工作目录字符串。

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 与 syscall 路径拷贝上限一致。
pub const PATH_MAX: usize = 256;

/// 全局 per-task cwd 表。
pub struct PerTaskCwdRegistry {
    cwd_tables: Vec<Option<String>>,
    owners: Vec<Option<task::TaskId>>,
    ref_counts: Vec<usize>,
}

impl PerTaskCwdRegistry {
    pub const fn new() -> Self {
        Self {
            cwd_tables: Vec::new(),
            owners: Vec::new(),
            ref_counts: Vec::new(),
        }
    }

    fn slot_mut(&mut self, task_id: task::TaskId) -> &mut Option<String> {
        if self.cwd_tables.len() <= task_id {
            self.cwd_tables.resize_with(task_id + 1, || None);
            self.owners.resize_with(task_id + 1, || None);
            self.ref_counts.resize(task_id + 1, 0);
        }
        if self.owners[task_id].is_none() {
            self.owners[task_id] = Some(task_id);
            self.ref_counts[task_id] = 1;
        }
        let owner = self.effective_owner(task_id);
        &mut self.cwd_tables[owner]
    }

    fn effective_owner(&self, task_id: task::TaskId) -> task::TaskId {
        self.owners
            .get(task_id)
            .and_then(|owner| *owner)
            .unwrap_or(task_id)
    }

    fn release_owner(&mut self, task_id: task::TaskId) -> Option<task::TaskId> {
        let owner = self.owners.get_mut(task_id)?.take()?;
        if owner < self.ref_counts.len() && self.ref_counts[owner] > 0 {
            self.ref_counts[owner] -= 1;
        }
        Some(owner)
    }

    /// 新任务默认工作目录为 `/`。
    pub fn init_task_cwd(&mut self, task_id: task::TaskId) {
        *self.slot_mut(task_id) = Some(String::from("/"));
    }

    /// 无记录时视为 `/`（不自动分配槽位）。
    pub fn get_cwd(&self, task_id: task::TaskId) -> &str {
        let owner = self.effective_owner(task_id);
        self.cwd_tables
            .get(owner)
            .and_then(|o| o.as_deref())
            .unwrap_or("/")
    }

    /// 确保任务已有 cwd 槽位（默认 `/`）。
    pub fn ensure_task_cwd(&mut self, task_id: task::TaskId) {
        let owner = self.effective_owner(task_id);
        if self.cwd_tables.get(owner).and_then(|o| o.as_ref()).is_none() {
            self.init_task_cwd(task_id);
        }
    }

    /// 取当前任务 cwd 的可变引用（调用方需已持有注册表锁且已 `ensure_task_cwd`）。
    pub fn get_cwd_mut(&mut self, task_id: task::TaskId) -> &mut String {
        self.ensure_task_cwd(task_id);
        self.slot_mut(task_id).as_mut().expect("init_task_cwd")
    }

    /// 任务退出后丢弃槽位，避免 `TaskId` 复用污染。
    pub fn drop_task(&mut self, task_id: task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts.get(owner).copied().unwrap_or(0) == 0 {
            if let Some(slot) = self.cwd_tables.get_mut(owner) {
                *slot = None;
            }
        }
        if task_id != owner && task_id < self.cwd_tables.len() {
            self.cwd_tables[task_id] = None;
        }
    }

    /// 供未来 `fork`/`clone`：复制父任务 cwd。
    pub fn copy_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        let parent_cwd = self.get_cwd(parent).to_string();
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task(child);
        }
        if parent_cwd.len() >= PATH_MAX {
            *self.slot_mut(child) = Some(String::from("/"));
        } else {
            *self.slot_mut(child) = Some(parent_cwd);
        }
    }

    /// thread clone 时共享父任务 cwd。
    pub fn share_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task_cwd(parent);
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task(child);
        }
        if self.cwd_tables.len() <= child {
            self.cwd_tables.resize_with(child + 1, || None);
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
