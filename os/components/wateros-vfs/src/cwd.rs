//! per-task 工作目录：全局注册表、`chdir`/`getcwd` 支撑与 `open` 路径解析。

#![cfg(feature = "impl-fd-session")]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

use api_v0::{
    normalize_absolute_path, register_open_path_resolver, resolve_against_cwd,
    SingleRootReadView, VfsError, VfsNodeType, VfsResult,
};
use base::sync::MultiprocessorSafeCell;
use impl_fd_session::{PerTaskCwdRegistry, PATH_MAX};

use crate::root;

static mut CWD_REGISTRY: MaybeUninit<MultiprocessorSafeCell<PerTaskCwdRegistry>> = MaybeUninit::uninit();
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
pub fn registry() -> &'static MultiprocessorSafeCell<PerTaskCwdRegistry> {
    if CWD_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            CWD_REGISTRY.write(MultiprocessorSafeCell::new(PerTaskCwdRegistry::new()));
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
    if !path_within_root(abs.as_str(), reg.get_root(task_id)) {
        return Err(VfsError::AccessDenied);
    }
    *reg.get_cwd_mut(task_id) = abs;
    Ok(())
}

/// 根据根卷 ELF 路径（如 `/glibc/basic/read`）将任务 cwd 设为所在目录，并记录 exe/argv。
///
/// `busybox sh /path/script.sh` 时将 cwd 设为脚本目录，便于脚本内 `. foo.sh` 相对 source。
pub fn on_user_task_spawned_for_elf(
    task_id: task::TaskId,
    elf_vfs_path: &str,
    argv: &[&str],
) {
    init_task_cwd(task_id);
    let _ = set_task_exe_path(task_id, elf_vfs_path);
    let _ = set_task_argv(task_id, argv.iter().copied());
    let cwd = initial_cwd_for_spawn(elf_vfs_path, argv);
    let _ = set_task_cwd(task_id, cwd.as_str());
}

// spawn 时 cwd 启发式：shell 脚本路径优先于 ELF 所在目录。
fn initial_cwd_for_spawn(elf_vfs_path: &str, argv: &[&str]) -> String {
    if argv.len() >= 2 && is_shell_invocation(argv[0]) {
        if let Some(dir) = parent_dir(argv[1]) {
            return String::from(dir);
        }
    }
    if let Some(arg0) = argv.first() {
        if arg0.ends_with(".sh") {
            if let Some(dir) = parent_dir(arg0) {
                return String::from(dir);
            }
        }
    }
    String::from(parent_dir(elf_vfs_path).unwrap_or("/"))
}

fn is_shell_invocation(argv0: &str) -> bool {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    matches!(name, "sh" | "bash" | "dash" | "ash" | "busybox")
}

// 取路径父目录；根下文件返回 `/`。
fn parent_dir(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(dir, _)| if dir.is_empty() { "/" } else { dir })
}

/// 记录任务当前可执行文件路径，供 `/proc/self/exe` 兼容路径使用。
pub fn set_task_exe_path(task_id: task::TaskId, exe_path: &str) -> VfsResult<()> {
    if exe_path.is_empty() || exe_path.len() >= PATH_MAX {
        return Err(VfsError::InvalidPath);
    }
    let abs = String::from(normalize_absolute_path(exe_path)?.as_str());
    if abs.len() >= PATH_MAX {
        return Err(VfsError::InvalidPath);
    }
    let mut reg = registry().exclusive_access();
    reg.set_exe_path(task_id, abs.as_str());
    Ok(())
}

/// 记录任务 argv，供 `/proc/<pid>/cmdline` 使用。
pub fn set_task_argv<I>(task_id: task::TaskId, argv: I) -> VfsResult<()>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let argv: Vec<String> = argv
        .into_iter()
        .map(|s| String::from(s.as_ref()))
        .collect();
    let mut reg = registry().exclusive_access();
    reg.set_argv(task_id, argv);
    Ok(())
}

/// 读取指定任务的 argv。
pub fn task_argv(task_id: task::TaskId) -> VfsResult<Vec<String>> {
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    reg.get_argv(task_id)
        .map(|v| v.to_vec())
        .ok_or(VfsError::NotFound)
}

/// 读取当前任务 argv；无记录时返回空 vec。
pub fn current_argv() -> Vec<String> {
    let task_id = match crate::fd::current_task_id() {
        Ok(id) => id,
        Err(_) => return Vec::new(),
    };
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    reg.get_argv(task_id).map(|v| v.to_vec()).unwrap_or_default()
}

/// 读取指定任务 argv（procfs 回调用）。
pub fn lookup_argv_for_task(task_id: task::TaskId) -> Option<Vec<String>> {
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    reg.get_argv(task_id).map(|v| v.to_vec())
}

/// 读取指定任务 exe 路径（procfs 回调用）。
pub fn lookup_exe_for_task(task_id: task::TaskId) -> Option<String> {
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    reg.get_exe_path(task_id).map(String::from)
}

/// 读取指定任务的逻辑 cwd（procfs 回调用）。
pub fn lookup_cwd_for_task(task_id: task::TaskId) -> Option<String> {
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    Some(String::from(reg.get_cwd(task_id)))
}

/// 读取指定任务的进程根目录（procfs 回调用）。
pub fn lookup_root_for_task(task_id: task::TaskId) -> Option<String> {
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    Some(String::from(reg.get_root(task_id)))
}

/// 读取当前任务的可执行文件路径。
pub fn current_exe_path() -> VfsResult<String> {
    let task_id = crate::fd::current_task_id()?;
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    reg.get_exe_path(task_id)
        .map(String::from)
        .ok_or(VfsError::NotFound)
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
    let root = reg.get_root(task_id);
    resolve_with_root(root, cwd, path)
}

fn path_within_root(path: &str, root: &str) -> bool {
    root == "/" || path == root || path.starts_with(root) &&
                                      path.as_bytes().get(root.len()) == Some(&b'/')
}

fn logical_path<'a>(path: &'a str, root: &str) -> VfsResult<&'a str> {
    if !path_within_root(path, root) {
        return Err(VfsError::AccessDenied);
    }
    if root == "/" {
        Ok(path)
    } else if path == root {
        Ok("/")
    } else {
        Ok(&path[root.len()..])
    }
}

fn resolve_with_root(root: &str, base: &str, path: &str) -> VfsResult<String> {
    let logical_base = logical_path(base, root)?;
    let logical = resolve_against_cwd(logical_base, Some(path))?;
    if root == "/" {
        Ok(logical)
    } else if logical == "/" {
        Ok(String::from(root))
    } else {
        Ok(alloc::format!("{}{}", root, logical))
    }
}

pub fn resolve_with_virtual_root(root: &str, base: &str, path: &str) -> VfsResult<String> {
    resolve_with_root(root, base, path)
}

/// 以当前任务 root 约束一个已打开目录 fd 的物理路径。
pub fn resolve_from_directory(path: &str, relative: &str) -> VfsResult<String> {
    let task_id = crate::fd::current_task_id()?;
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    resolve_with_root(reg.get_root(task_id), path, relative)
}

pub fn current_root() -> VfsResult<String> {
    let task_id = crate::fd::current_task_id()?;
    let mut reg = registry().exclusive_access();
    reg.ensure_task_cwd(task_id);
    Ok(String::from(reg.get_root(task_id)))
}

/// 将当前任务 root 与 cwd 同时切换到已解析的物理目录。
pub fn chroot_current(root_path: &str) -> VfsResult<()> {
    let resolved = resolve_for_current_task(root_path)?;
    chroot_current_resolved(resolved.as_str())
}

pub fn chroot_current_resolved(resolved: &str) -> VfsResult<()> {
    let task_id = crate::fd::current_task_id()?;
    let current_root = current_root()?;
    if !path_within_root(resolved, current_root.as_str()) {
        return Err(VfsError::AccessDenied);
    }
    let meta = root::read_view().metadata(resolved)?;
    if meta.node_type != VfsNodeType::Directory {
        return Err(VfsError::NotDirectory);
    }
    let mut reg = registry().exclusive_access();
    reg.set_root(task_id, String::from(resolved));
    *reg.get_cwd_mut(task_id) = String::from(resolved);
    Ok(())
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
        resolve_with_root(reg.get_root(task_id), cwd, path)?
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
    let cwd = logical_path(reg.get_cwd(task_id), reg.get_root(task_id))?;
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
    reg.set_exe_path(a, "/glibc/basic/read");
    reg.set_argv(a, vec![String::from("read"), String::from("arg1")]);
    assert_eq!(reg.get_exe_path(c), Some("/glibc/basic/read"));
    assert_eq!(
        reg.get_argv(c).map(|v| v.to_vec()),
        Some(vec![String::from("read"), String::from("arg1")])
    );
    reg.copy_cwd_from_parent(b, a);
    assert_eq!(
        reg.get_argv(b).map(|v| v.to_vec()),
        Some(vec![String::from("read"), String::from("arg1")])
    );
    reg.drop_task(c);
    assert_eq!(reg.get_cwd(a), "/glibc/basic");
    reg.drop_task(a);
    assert_eq!(reg.get_cwd(a), "/");

    let resolved = resolve_against_cwd("/glibc/basic", Some("./text.txt")).expect("resolve");
    assert_eq!(resolved, "/glibc/basic/text.txt");
    assert_eq!(
        initial_cwd_for_spawn(
            "/bin/bash",
            &["/bin/bash", "/glibc/cagent_testcode.sh"],
        ),
        "/glibc"
    );
    assert_eq!(
        initial_cwd_for_spawn("/glibc/busybox", &["sh", "/glibc/basic_testcode.sh"]),
        "/glibc"
    );
}
