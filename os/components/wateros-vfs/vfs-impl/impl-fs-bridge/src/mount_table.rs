//! 辅助卷挂载表（最长前缀路由）；支持 RW 与 RO 辅助挂载。

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use fs::{SharedFs, SharedRwFs};
use spin::Mutex;

use api_v0::{normalize_absolute_path, VfsDirEntry, VfsError, VfsResult};

/// 辅助挂载句柄：RW 或 RO。
pub(crate) enum AuxMount {
    Rw(SharedRwFs),
    Ro(SharedFs),
}

struct MountEntry {
    mount_point: String,
    fs: AuxMount,
}

static AUX_MOUNTS: Mutex<Vec<MountEntry>> = Mutex::new(Vec::new());

/// 路径路由：根卷相对路径，或辅助卷 + 卷内相对路径。
pub(crate) enum FsRoute {
    Root { abs: String },
    AuxRw { fs: SharedRwFs, rel: String },
    AuxRo { fs: SharedFs, rel: String },
}

fn rel_under_mount(full: &str, mount_point: &str) -> String {
    if full == mount_point {
        return String::from("/");
    }
    let rest = full.strip_prefix(mount_point).unwrap_or(full);
    if rest.is_empty() {
        String::from("/")
    } else if rest.starts_with('/') {
        String::from(rest)
    } else {
        alloc::format!("/{}", rest)
    }
}

fn longest_aux_mount(abs: &str) -> Option<(AuxMount, String)> {
    let table = AUX_MOUNTS.lock();
    let mut best: Option<(usize, AuxMount, String)> = None;
    for ent in table.iter() {
        let mp = ent.mount_point.as_str();
        let matches = abs == mp || abs.starts_with(mp) && abs.as_bytes().get(mp.len()) == Some(&b'/');
        if !matches {
            continue;
        }
        let len = mp.len();
        if best.as_ref().map(|(l, _, _)| len > *l).unwrap_or(true) {
            best = Some((len, ent.fs.clone_mount(), String::from(mp)));
        }
    }
    best.map(|(_, fs, mp)| (fs, rel_under_mount(abs, mp.as_str())))
}

impl AuxMount {
    fn clone_mount(&self) -> Self {
        match self {
            Self::Rw(fs) => Self::Rw(fs.clone()),
            Self::Ro(fs) => Self::Ro(fs.clone()),
        }
    }

}

/// 挂载点目录是否仅含 `.` / `..`（oscomp 预置 `mnt` 目录可能非空展示项）。
fn mount_point_dir_is_empty(entries: &[VfsDirEntry]) -> bool {
    entries
        .iter()
        .all(|e| e.name == "." || e.name == "..")
}

fn mount_aux_common(mount_point: &str, fs: AuxMount) -> VfsResult<()> {
    let mp = String::from(normalize_absolute_path(mount_point)?.as_str());
    if mp == "/" {
        return Err(VfsError::InvalidPath);
    }
    {
        let table = AUX_MOUNTS.lock();
        if table.iter().any(|e| e.mount_point == mp) {
            return Err(VfsError::Exists);
        }
    }
    let entries = super::FsBridge::read_dir_on_root(mp.as_str())?;
    if !mount_point_dir_is_empty(entries.as_slice()) {
        return Err(VfsError::Exists);
    }
    AUX_MOUNTS.lock().push(MountEntry {
        mount_point: mp,
        fs,
    });
    fs::rootfs::active_impl::bump_mount_generation();
    Ok(())
}

pub(crate) fn resolve_route(path: &str) -> VfsResult<FsRoute> {
    let abs = String::from(normalize_absolute_path(path)?.as_str());
    if let Some((mount, rel)) = longest_aux_mount(abs.as_str()) {
        return match mount {
            AuxMount::Rw(fs) => Ok(FsRoute::AuxRw { fs, rel }),
            AuxMount::Ro(fs) => Ok(FsRoute::AuxRo { fs, rel }),
        };
    }
    Ok(FsRoute::Root { abs })
}

/// 写路径、带 `O_CREAT`/`O_WRONLY` 的 open 等须先调用；RO 辅助卷返回 [`VfsError::ReadOnlyFs`]。
pub(crate) fn assert_path_writable(path: &str) -> VfsResult<()> {
    match resolve_route(path)? {
        FsRoute::AuxRo { .. } => Err(VfsError::ReadOnlyFs),
        _ => Ok(()),
    }
}

/// 将 RW 句柄挂到 `mount_point`（须为根卷上的空目录）。
pub(crate) fn mount_aux_at_rw(mount_point: &str, fs: SharedRwFs) -> VfsResult<()> {
    mount_aux_common(mount_point, AuxMount::Rw(fs))
}

/// 将 RO 句柄挂到 `mount_point`（须为根卷上的空目录）。
pub(crate) fn mount_aux_at_ro(mount_point: &str, fs: SharedFs) -> VfsResult<()> {
    mount_aux_common(mount_point, AuxMount::Ro(fs))
}

pub(crate) fn unmount_aux_at(mount_point: &str) -> VfsResult<()> {
    let mp = String::from(normalize_absolute_path(mount_point)?.as_str());
    if mp == "/" {
        return Err(VfsError::InvalidPath);
    }
    let mut table = AUX_MOUNTS.lock();
    let pos = table
        .iter()
        .position(|e| e.mount_point == mp)
        .ok_or(VfsError::NotFound)?;
    table.remove(pos);
    fs::rootfs::active_impl::bump_mount_generation();
    Ok(())
}

/// 挂载表逻辑自测（不依赖第二块盘）：注册/解析/卸载。
pub fn mount_table_self_test() -> VfsResult<()> {
    let n_before = AUX_MOUNTS.lock().len();
    let root = super::root_rw()?;
    let mp = "/__bringup_mount_test__";
    mount_aux_at_rw(mp, root.clone())?;
    let probe = alloc::format!("{mp}/x");
    let route = resolve_route(probe.as_str())?;
    match route {
        FsRoute::AuxRw { rel, .. } if rel == "/x" => {}
        _ => return Err(VfsError::Io),
    }
    unmount_aux_at(mp)?;
    if AUX_MOUNTS.lock().len() != n_before {
        return Err(VfsError::Io);
    }
    Ok(())
}
