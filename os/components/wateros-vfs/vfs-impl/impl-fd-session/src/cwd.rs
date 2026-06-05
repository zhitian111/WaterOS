//! 按 [`task::TaskId`] 索引的 per-task 工作目录字符串。

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 与 syscall 路径拷贝上限一致。
pub const PATH_MAX: usize = 256;

/// 全局 per-task cwd 表。
pub struct PerTaskCwdRegistry {
    cwd_tables: Vec<Option<String>>,
    exe_paths: Vec<Option<String>>,
    argv_vectors: Vec<Option<Vec<String>>>,
    owners: Vec<Option<task::TaskId>>,
    ref_counts: Vec<usize>,
}

impl PerTaskCwdRegistry {
    pub const fn new() -> Self {
        Self {
            cwd_tables: Vec::new(),
            exe_paths: Vec::new(),
            argv_vectors: Vec::new(),
            owners: Vec::new(),
            ref_counts: Vec::new(),
        }
    }

    fn slot_mut(&mut self, task_id: task::TaskId) -> &mut Option<String> {
        if self.cwd_tables.len() <= task_id {
            self.cwd_tables.resize_with(task_id + 1, || None);
            self.exe_paths.resize_with(task_id + 1, || None);
            self.argv_vectors.resize_with(task_id + 1, || None);
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

    pub fn init_task_cwd(&mut self, task_id: task::TaskId) {
        *self.slot_mut(task_id) = Some(String::from("/"));
    }

    pub fn get_cwd(&self, task_id: task::TaskId) -> &str {
        let owner = self.effective_owner(task_id);
        self.cwd_tables
            .get(owner)
            .and_then(|o| o.as_deref())
            .unwrap_or("/")
    }

    pub fn ensure_task_cwd(&mut self, task_id: task::TaskId) {
        let owner = self.effective_owner(task_id);
        if self.cwd_tables.get(owner).and_then(|o| o.as_ref()).is_none() {
            self.init_task_cwd(task_id);
        }
    }

    pub fn get_cwd_mut(&mut self, task_id: task::TaskId) -> &mut String {
        self.ensure_task_cwd(task_id);
        self.slot_mut(task_id).as_mut().expect("init_task_cwd")
    }

    pub fn drop_task(&mut self, task_id: task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts.get(owner).copied().unwrap_or(0) == 0 {
            if let Some(slot) = self.cwd_tables.get_mut(owner) {
                *slot = None;
            }
            if let Some(slot) = self.exe_paths.get_mut(owner) {
                *slot = None;
            }
            if let Some(slot) = self.argv_vectors.get_mut(owner) {
                *slot = None;
            }
        }
        if task_id != owner && task_id < self.cwd_tables.len() {
            self.cwd_tables[task_id] = None;
            self.exe_paths[task_id] = None;
            self.argv_vectors[task_id] = None;
        }
    }

    pub fn copy_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        let parent_cwd = self.get_cwd(parent).to_string();
        let parent_exe = self.get_exe_path(parent).map(ToString::to_string);
        let parent_argv = self.get_argv(parent).map(|v| v.to_vec());
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task(child);
        }
        if parent_cwd.len() >= PATH_MAX {
            *self.slot_mut(child) = Some(String::from("/"));
        } else {
            *self.slot_mut(child) = Some(parent_cwd);
        }
        let child_owner = self.effective_owner(child);
        if let Some(exe) = parent_exe {
            self.exe_paths[child_owner] = Some(exe);
        }
        if let Some(argv) = parent_argv {
            self.argv_vectors[child_owner] = Some(argv);
        }
    }

    pub fn share_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task_cwd(parent);
        if self.owners.get(child).and_then(|owner| *owner).is_some() {
            self.drop_task(child);
        }
        if self.cwd_tables.len() <= child {
            self.cwd_tables.resize_with(child + 1, || None);
            self.exe_paths.resize_with(child + 1, || None);
            self.argv_vectors.resize_with(child + 1, || None);
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

    pub fn set_exe_path(&mut self, task_id: task::TaskId, exe_path: &str) {
        let _ = self.slot_mut(task_id);
        let owner = self.effective_owner(task_id);
        self.exe_paths[owner] = Some(String::from(exe_path));
    }

    pub fn get_exe_path(&self, task_id: task::TaskId) -> Option<&str> {
        let owner = self.effective_owner(task_id);
        self.exe_paths
            .get(owner)
            .and_then(|path| path.as_deref())
    }

    pub fn set_argv(&mut self, task_id: task::TaskId, argv: Vec<String>) {
        let _ = self.slot_mut(task_id);
        let owner = self.effective_owner(task_id);
        self.argv_vectors[owner] = Some(argv);
    }

    pub fn get_argv(&self, task_id: task::TaskId) -> Option<&[String]> {
        let owner = self.effective_owner(task_id);
        self.argv_vectors
            .get(owner)
            .and_then(|v| v.as_deref())
    }
}
