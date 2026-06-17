//! 辅助卷挂载表（最长前缀路由）；支持 RW、RO 与 procfs 伪挂载。

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use fs::{SharedFs, SharedRwFs};
use fs::procfs::api::ProcMountLine;
use spin::Mutex;

use api_v0::{normalize_absolute_path, VfsError, VfsResult};

/// 辅助挂载句柄：RW、RO 或 procfs 伪挂载。
pub(crate) enum AuxMount {
    Rw(SharedRwFs),
    Ro(SharedFs),
    PseudoProc,
}

struct MountEntry {
    mount_point: String,
    fs: AuxMount,
    identity: MountIdentity,
}

static AUX_MOUNTS: Mutex<Vec<MountEntry>> = Mutex::new(Vec::new());
static DEVICE_IDS: Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());
static NEXT_DEVICE_MINOR: AtomicU64 = AtomicU64::new(1);
static NEXT_MOUNT_ID: AtomicU64 = AtomicU64::new(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MountIdentity {
    pub device_major: u32,
    pub device_minor: u32,
    pub mount_id: u64,
}

fn device_minor_for(key: &str) -> u32 {
    let mut devices = DEVICE_IDS.lock();
    if let Some((_, minor)) = devices.iter().find(|(known, _)| known == key) {
        return *minor;
    }
    let minor = u32::try_from(NEXT_DEVICE_MINOR.fetch_add(1, Ordering::Relaxed))
        .expect("VFS device id exhausted");
    devices.push((String::from(key), minor));
    minor
}

fn new_mount_identity(device_key: &str) -> MountIdentity {
    MountIdentity {
        device_major: 0,
        device_minor: device_minor_for(device_key),
        mount_id: NEXT_MOUNT_ID.fetch_add(1, Ordering::Relaxed),
    }
}

pub(crate) fn root_identity() -> MountIdentity {
    let device = fs::rootfs::active_impl::current_root_device_path()
        .unwrap_or_else(|| String::from("/dev/root"));
    MountIdentity {
        device_major: 0,
        device_minor: device_minor_for(device.as_str()),
        mount_id: 1,
    }
}

/// 路径路由：根卷相对路径，或辅助卷 + 卷内相对路径，或 procfs 伪挂载。
pub(crate) enum FsRoute {
    Root { abs: String, identity: MountIdentity },
    AuxRw { fs: SharedRwFs, rel: String, identity: MountIdentity },
    AuxRo { fs: SharedFs, rel: String, identity: MountIdentity },
    PseudoProc { rel: String, identity: MountIdentity },
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

fn longest_aux_mount(abs: &str) -> Option<(AuxMount, MountIdentity, String)> {
    let table = AUX_MOUNTS.lock();
    let mut best: Option<(usize, AuxMount, MountIdentity, String)> = None;
    for ent in table.iter() {
        let mp = ent.mount_point.as_str();
        let matches = abs == mp || abs.starts_with(mp) && abs.as_bytes().get(mp.len()) == Some(&b'/');
        if !matches {
            continue;
        }
        let len = mp.len();
        if best.as_ref().map(|(l, _, _, _)| len > *l).unwrap_or(true) {
            best = Some((len, ent.fs.clone_mount(), ent.identity, String::from(mp)));
        }
    }
    best.map(|(_, fs, identity, mp)| (fs, identity, rel_under_mount(abs, mp.as_str())))
}

impl AuxMount {
    fn clone_mount(&self) -> Self {
        match self {
            Self::Rw(fs) => Self::Rw(fs.clone()),
            Self::Ro(fs) => Self::Ro(fs.clone()),
            Self::PseudoProc => Self::PseudoProc,
        }
    }
}

fn mount_aux_common(mount_point: &str, fs: AuxMount, device_key: &str) -> VfsResult<()> {
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
    let _ = super::FsBridge::read_dir_on_root(mp.as_str())?;
    AUX_MOUNTS.lock().push(MountEntry {
        mount_point: mp,
        fs,
        identity: new_mount_identity(device_key),
    });
    fs::rootfs::active_impl::bump_mount_generation();
    Ok(())
}

pub(crate) fn resolve_route(path: &str) -> VfsResult<FsRoute> {
    let abs = String::from(normalize_absolute_path(path)?.as_str());
    if let Some((mount, identity, rel)) = longest_aux_mount(abs.as_str()) {
        return match mount {
            AuxMount::Rw(fs) => Ok(FsRoute::AuxRw { fs, rel, identity }),
            AuxMount::Ro(fs) => Ok(FsRoute::AuxRo { fs, rel, identity }),
            AuxMount::PseudoProc => Ok(FsRoute::PseudoProc { rel, identity }),
        };
    }
    Ok(FsRoute::Root { abs, identity: root_identity() })
}

/// 写路径、带 `O_CREAT`/`O_WRONLY` 的 open 等须先调用；RO / procfs 返回 [`VfsError::ReadOnlyFs`]。
pub(crate) fn assert_path_writable(path: &str) -> VfsResult<()> {
    match resolve_route(path)? {
        FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } => Err(VfsError::ReadOnlyFs),
        _ => Ok(()),
    }
}

pub(crate) fn mount_aux_at_rw(mount_point: &str, fs: SharedRwFs, device_key: &str) -> VfsResult<()> {
    mount_aux_common(mount_point, AuxMount::Rw(fs), device_key)
}

pub(crate) fn mount_aux_at_ro(mount_point: &str, fs: SharedFs, device_key: &str) -> VfsResult<()> {
    mount_aux_common(mount_point, AuxMount::Ro(fs), device_key)
}

pub fn mount_aux_proc_at(mount_point: &str) -> VfsResult<()> {
    mount_aux_common(mount_point, AuxMount::PseudoProc, "proc")
}

pub fn is_proc_mounted_at(mount_point: &str) -> bool {
    let Ok(mp) = normalize_absolute_path(mount_point) else {
        return false;
    };
    AUX_MOUNTS
        .lock()
        .iter()
        .any(|e| e.mount_point == mp.as_str() && matches!(e.fs, AuxMount::PseudoProc))
}

fn fstype_for(entry: &MountEntry) -> &'static str {
    match entry.fs {
        AuxMount::PseudoProc => "proc",
        AuxMount::Rw(_) | AuxMount::Ro(_) => "ext4",
    }
}

fn device_for(entry: &MountEntry) -> String {
    match entry.fs {
        AuxMount::PseudoProc => String::from("proc"),
        AuxMount::Rw(_) | AuxMount::Ro(_) => entry.mount_point.clone(),
    }
}

fn root_mount_device() -> String {
    fs::devfs::active_impl::default_root_block_path().unwrap_or_else(|| String::from("/dev/root"))
}

pub fn list_proc_mount_lines() -> Vec<ProcMountLine> {
    let mut out = Vec::new();
    if fs::rootfs::active_impl::root_rw_fs().is_some() {
        out.push(ProcMountLine {
            device: root_mount_device(),
            mount_point: String::from("/"),
            fstype: String::from("ext4"),
        });
    }
    for ent in AUX_MOUNTS.lock().iter() {
        out.push(ProcMountLine {
            device: device_for(ent),
            mount_point: ent.mount_point.clone(),
            fstype: String::from(fstype_for(ent)),
        });
    }
    out
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

pub fn mount_table_self_test() -> VfsResult<()> {
    let dev_a = new_mount_identity("/dev/__identity_test__");
    let dev_b = new_mount_identity("/dev/__identity_test__");
    assert_eq!(dev_a.device_major, dev_b.device_major);
    assert_eq!(dev_a.device_minor, dev_b.device_minor);
    assert_ne!(dev_a.mount_id, dev_b.mount_id);

    let n_before = AUX_MOUNTS.lock().len();
    let root = super::root_rw()?;
    let mp = "/__bringup_mount_test__";
    mount_aux_at_rw(mp, root.clone(), "/dev/root-self-test")?;
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
