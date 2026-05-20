//! 按 [`task::TaskId`] 索引的 per-task 工作目录字符串。

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 与 syscall 路径拷贝上限一致。
pub const PATH_MAX: usize = 256;

/// 全局 per-task cwd 表。
pub struct PerTaskCwdRegistry {
    cwd_tables: Vec<Option<String>>,
}

impl PerTaskCwdRegistry {
    pub const fn new() -> Self {
        Self { cwd_tables: Vec::new() }
    }

    fn slot_mut(&mut self, task_id: task::TaskId) -> &mut Option<String> {
        if self.cwd_tables.len() <= task_id {
            self.cwd_tables.resize_with(task_id + 1, || None);
        }
        &mut self.cwd_tables[task_id]
    }

    /// 新任务默认工作目录为 `/`。
    pub fn init_task_cwd(&mut self, task_id: task::TaskId) {
        *self.slot_mut(task_id) = Some(String::from("/"));
    }

    /// 无记录时视为 `/`（不自动分配槽位）。
    pub fn get_cwd(&self, task_id: task::TaskId) -> &str {
        self.cwd_tables
            .get(task_id)
            .and_then(|o| o.as_deref())
            .unwrap_or("/")
    }

    /// 确保任务已有 cwd 槽位（默认 `/`）。
    pub fn ensure_task_cwd(&mut self, task_id: task::TaskId) {
        if self.cwd_tables.get(task_id).and_then(|o| o.as_ref()).is_none() {
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
        if let Some(slot) = self.cwd_tables.get_mut(task_id) {
            *slot = None;
        }
    }

    /// 供未来 `fork`/`clone`：复制父任务 cwd。
    pub fn copy_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        let parent_cwd = self.get_cwd(parent).to_string();
        if parent_cwd.len() >= PATH_MAX {
            *self.slot_mut(child) = Some(String::from("/"));
        } else {
            *self.slot_mut(child) = Some(parent_cwd);
        }
    }
}
