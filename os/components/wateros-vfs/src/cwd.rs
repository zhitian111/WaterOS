//! per-task 工作目录：全局注册表、`chdir`/`getcwd` 支撑与 `open` 路径解析。

#![cfg(feature = "fd-session")]

extern crate alloc;

use alloc::string::String;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{
    normalize_absolute_path, register_open_path_resolver, resolve_against_cwd,
    SingleRootReadView, VfsError, VfsNodeType, VfsResult,
};
use base::sync::UniprocessorSafeCell;
use impl_fd_session::{PerTaskCwdRegistry, PATH_MAX};

use crate::root;

static mut CWD_REGISTRY: MaybeUninit<UniprocessorSafeCell<PerTaskCwdRegistry>> = MaybeUninit::uninit();
static CWD_REGISTRY_READY: AtomicUsize = AtomicUsize::new(0);
static OPEN_RESOLVER_REGISTERED: AtomicUsize = AtomicUsize::new(0);

fn ensure_open_path_resolver_registered() {
    if OPEN_RESOLVER_REGISTERED.load(Ordering::Acquire) != 0 {
        return;
    }
    register_open_path_resolver(resolve_for_current_task);
    OPEN_RESOLVER_REGISTERED.store(1, Ordering::Release);
}

/// 全局 per-task cwd 注册表。
pub fn registry() -> &'static UniprocessorSafeCell<PerTaskCwdRegistry> {
    if CWD_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            CWD_REGISTRY.write(UniprocessorSafeCell::new(PerTaskCwdRegistry::new()));
        }
        CWD_REGISTRY_READY.store(1, Ordering::Release);
        ensure_open_path_resolver_registered();
    }
    unsafe { &*CWD_REGISTRY.as_ptr() }
}

/// 新用户/内核任务分配 id 后初始化 cwd 为 `/`。
pub fn init_task_cwd(task_id: task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.init_task_cwd(task_id);
}

/// `spawn_user_task_*` 返回后调用，显式建立 cwd（与惰性 `ensure_task_cwd` 等价）。
pub fn on_user_task_spawned(task_id: task::TaskId) {
    init_task_cwd(task_id);
}

/// 将指定任务 cwd 设为已存在的绝对目录路径（bring-up / exec 前使用）。
pub fn set_task_cwd(task_id: task::TaskId, cwd: &str) -> VfsResult<()> {
    if cwd.is_empty() || cwd.len() >= PATH_MAX {
        return Err(VfsError::InvalidPath);
    }
    let abs = String::from(normalize_absolute_path(cwd)?.as_str());
    if abs.len() >= PATH_MAX {
        return Err(VfsError::InvalidPath);
    }
    let view = root::read_view();
    if !view.exists(abs.as_str())? {
        return Err(VfsError::NotFound);
    }
    let meta = view.metadata(abs.as_str())?;
    if meta.node_type != VfsNodeType::Directory {
        return Err(VfsError::NotAFile);
    }
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    *reg.get_cwd_mut(task_id) = abs;
    Ok(())
}

/// 根据根卷 ELF 路径（如 `/glibc/basic/read`）将任务 cwd 设为所在目录。
pub fn on_user_task_spawned_for_elf(task_id: task::TaskId, elf_vfs_path: &str) {
    init_task_cwd(task_id);
    if let Some((dir, _)) = elf_vfs_path.rsplit_once('/') {
        let cwd = if dir.is_empty() { "/" } else { dir };
        let _ = set_task_cwd(task_id, cwd);
    }
}

/// 任务回收后丢弃 cwd 槽位。
pub fn drop_task_cwd(task_id: task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.drop_task(task_id);
}

/// 供未来 `fork`/`clone` 复制父任务 cwd。
pub fn copy_cwd_from_parent(child: task::TaskId, parent: task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.copy_cwd_from_parent(child, parent);
}

/// thread clone 时共享父任务 cwd。
pub fn share_cwd_from_parent(child: task::TaskId, parent: task::TaskId) {
    let mut reg = registry().exclusive_access();
    reg.share_cwd_from_parent(child, parent);
}

/// 将 `path` 相对当前任务 cwd 解析为绝对路径。
pub fn resolve_for_current_task(path: &str) -> VfsResult<String> {
    let task_id = crate::fd::current_task_id()?;
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    let cwd = reg.get_cwd(task_id);
    resolve_against_cwd(cwd, Some(path))
}

/// 切换当前任务工作目录（校验目标为已存在目录）。
pub fn chdir_current(path: &str) -> VfsResult<()> {
    if path.len() >= PATH_MAX {
        return Err(VfsError::InvalidPath);
    }
    let task_id = crate::fd::current_task_id()?;
    let abs = {
        let mut reg = registry().exclusive_access();
        reg.ensure_task_cwd(task_id);
        let cwd = reg.get_cwd(task_id);
        resolve_against_cwd(cwd, Some(path))?
    };
    if abs.len() >= PATH_MAX {
        return Err(VfsError::InvalidPath);
    }
    let view = root::read_view();
    if !view.exists(abs.as_str())? {
        return Err(VfsError::NotFound);
    }
    let meta = view.metadata(abs.as_str())?;
    if meta.node_type != VfsNodeType::Directory {
        return Err(VfsError::NotAFile);
    }
    let mut reg = registry().exclusive_access();
    *reg.get_cwd_mut(task_id) = abs;
    Ok(())
}

/// 将当前任务 cwd（含 NUL）写入 `buf`；`buf.len()` 须 ≥ `cwd.len() + 1`。
pub fn write_cwd_to_buf(buf: &mut [u8]) -> VfsResult<usize> {
    let task_id = crate::fd::current_task_id()?;
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    let cwd = reg.get_cwd(task_id);
    let need = cwd.len() + 1;
    if buf.len() < need {
        return Err(VfsError::InvalidPath);
    }
    buf[..cwd.len()].copy_from_slice(cwd.as_bytes());
    buf[cwd.len()] = 0;
    Ok(cwd.len() + 1)
}

/// bring-up：cwd 初始化、`chdir` 与路径解析烟囱。
pub fn self_test() {
    let mut reg = registry().exclusive_access();
    let a: task::TaskId = 20;
    let b: task::TaskId = 21;
    reg.init_task_cwd(a);
    reg.init_task_cwd(b);
    assert_eq!(reg.get_cwd(a), "/");
    reg.copy_cwd_from_parent(b, a);
    assert_eq!(reg.get_cwd(b), "/");
    *reg.get_cwd_mut(a) = String::from("/glibc/basic");
    let c: task::TaskId = 22;
    reg.share_cwd_from_parent(c, a);
    assert_eq!(reg.get_cwd(c), "/glibc/basic");
    reg.drop_task(c);
    assert_eq!(reg.get_cwd(a), "/glibc/basic");
    reg.drop_task(a);
    assert_eq!(reg.get_cwd(a), "/");

    let resolved = resolve_against_cwd("/glibc/basic", Some("./text.txt")).expect("resolve");
    assert_eq!(resolved, "/glibc/basic/text.txt");
}
