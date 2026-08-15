//! 以 [`task::TaskId`] 为 key 的 per-task 工作目录字符串。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 与 syscall 路径拷贝上限一致。
pub const PATH_MAX: usize = 256;

/// 全局 per-task cwd 表。
// 本结构代码由AI完成
pub struct PerTaskCwdRegistry {
    cwd_tables: BTreeMap<task::TaskId, String>,
    root_tables: BTreeMap<task::TaskId, String>,
    exe_paths: BTreeMap<task::TaskId, String>,
    argv_vectors: BTreeMap<task::TaskId, Vec<String>>,
    env_vectors: BTreeMap<task::TaskId, Vec<String>>,
    owners: BTreeMap<task::TaskId, task::TaskId>,
    ref_counts: BTreeMap<task::TaskId, usize>,
}

impl PerTaskCwdRegistry {
    pub const fn new() -> Self {
        Self {
            cwd_tables: BTreeMap::new(),
            root_tables: BTreeMap::new(),
            exe_paths: BTreeMap::new(),
            argv_vectors: BTreeMap::new(),
            env_vectors: BTreeMap::new(),
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

// 本方法代码由AI完成
    fn effective_owner(&self, task_id: task::TaskId) -> task::TaskId {
        self.owners
            .get(&task_id)
            .copied()
            .unwrap_or(task_id)
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

    /// 初始化任务 cwd 为 `/`（spawn / fork 路径）。
// 本方法代码由AI完成
    pub fn init_task_cwd(&mut self, task_id: task::TaskId) {
        let owner = self.ensure_owner(task_id);
        self.cwd_tables.insert(owner, String::from("/"));
        self.root_tables.insert(owner, String::from("/"));
    }

    /// 读取任务 cwd 字符串；未初始化时回退 `/`。
// 本方法代码由AI完成
    pub fn get_cwd(&self, task_id: task::TaskId) -> &str {
        let owner = self.effective_owner(task_id);
        self.cwd_tables
            .get(&owner)
            .map(String::as_str)
            .unwrap_or("/")
    }

    /// 惰性初始化 cwd 槽位（首次访问前调用）。
// 本方法代码由AI完成
    pub fn ensure_task_cwd(&mut self, task_id: task::TaskId) {
        let owner = self.ensure_owner(task_id);
        self.cwd_tables.entry(owner).or_insert_with(|| String::from("/"));
    }

// 本方法代码由AI完成
    pub fn get_cwd_mut(&mut self, task_id: task::TaskId) -> &mut String {
        self.ensure_task_cwd(task_id);
        let owner = self.effective_owner(task_id);
        self.cwd_tables.get_mut(&owner).expect("init_task_cwd")
    }

    pub fn get_root(&self, task_id: task::TaskId) -> &str {
        let owner = self.effective_owner(task_id);
        self.root_tables.get(&owner).map(String::as_str).unwrap_or("/")
    }

    pub fn set_root(&mut self, task_id: task::TaskId, root: String) {
        let owner = self.ensure_owner(task_id);
        self.root_tables.insert(owner, root);
    }

    /// 任务退出或 unshare 后释放 cwd / exe / argv / env 槽位。
// 本方法代码由AI完成
    pub fn drop_task(&mut self, task_id: task::TaskId) {
        let Some(owner) = self.release_owner(task_id) else {
            return;
        };
        if self.ref_counts.get(&owner).copied().unwrap_or(0) == 0 {
            self.cwd_tables.remove(&owner);
            self.root_tables.remove(&owner);
            self.exe_paths.remove(&owner);
            self.argv_vectors.remove(&owner);
            self.env_vectors.remove(&owner);
        }
        if task_id != owner {
            self.cwd_tables.remove(&task_id);
            self.root_tables.remove(&task_id);
            self.exe_paths.remove(&task_id);
            self.argv_vectors.remove(&task_id);
            self.env_vectors.remove(&task_id);
        }
    }

// 本方法代码由AI完成
    pub fn copy_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        let parent_cwd = self.get_cwd(parent).to_string();
        let parent_root = self.get_root(parent).to_string();
        let parent_exe = self.get_exe_path(parent).map(ToString::to_string);
        let parent_argv = self.get_argv(parent).map(|v| v.to_vec());
        let parent_env = self.get_env(parent).map(|v| v.to_vec());
        if self.owners.contains_key(&child) {
            self.drop_task(child);
        }
        let child_owner = self.ensure_owner(child);
        if parent_cwd.len() >= PATH_MAX {
            self.cwd_tables.insert(child_owner, String::from("/"));
        } else {
            self.cwd_tables.insert(child_owner, parent_cwd);
        }
        self.root_tables.insert(child_owner, parent_root);
        if let Some(exe) = parent_exe {
            self.exe_paths.insert(child_owner, exe);
        }
        if let Some(argv) = parent_argv {
            self.argv_vectors.insert(child_owner, argv);
        }
        if let Some(env) = parent_env {
            self.env_vectors.insert(child_owner, env);
        }
    }

// 本方法代码由AI完成
    pub fn share_cwd_from_parent(&mut self, child: task::TaskId, parent: task::TaskId) {
        self.ensure_task_cwd(parent);
        if self.owners.contains_key(&child) {
            self.drop_task(child);
        }
        let owner = self.effective_owner(parent);
        self.owners.insert(child, owner);
        let count = self.ref_counts.entry(owner).or_insert(0);
        *count = count.saturating_add(1);
    }

// 本方法代码由AI完成
    pub fn set_exe_path(&mut self, task_id: task::TaskId, exe_path: &str) {
        let owner = self.ensure_owner(task_id);
        self.exe_paths.insert(owner, String::from(exe_path));
    }

// 本方法代码由AI完成
    pub fn get_exe_path(&self, task_id: task::TaskId) -> Option<&str> {
        let owner = self.effective_owner(task_id);
        self.exe_paths
            .get(&owner)
            .map(String::as_str)
    }

// 本方法代码由AI完成
    pub fn set_argv(&mut self, task_id: task::TaskId, argv: Vec<String>) {
        let owner = self.ensure_owner(task_id);
        self.argv_vectors.insert(owner, argv);
    }

// 本方法代码由AI完成
    pub fn get_argv(&self, task_id: task::TaskId) -> Option<&[String]> {
        let owner = self.effective_owner(task_id);
        self.argv_vectors
            .get(&owner)
            .map(Vec::as_slice)
    }

    /// 保存 exec 时的环境向量；同一线程组共享该槽位。
    pub fn set_env(&mut self, task_id: task::TaskId, env: Vec<String>) {
        let owner = self.ensure_owner(task_id);
        self.env_vectors.insert(owner, env);
    }

    /// 读取最近一次成功 exec/spawn 时的环境向量。
    pub fn get_env(&self, task_id: task::TaskId) -> Option<&[String]> {
        let owner = self.effective_owner(task_id);
        self.env_vectors.get(&owner).map(Vec::as_slice)
    }
}
