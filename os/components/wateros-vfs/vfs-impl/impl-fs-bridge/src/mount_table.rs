//! 辅助卷挂载表（最长前缀路由）；支持 RW、RO、procfs/sysfs 伪挂载、bind 与传播类型。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use fs::procfs::api::ProcMountLine;
use fs::{FsNodeType, LocalRwFs, SharedFs, SharedRwFs};
use spin::Mutex;

use api_v0::{normalize_absolute_path, VfsError, VfsResult};
use base::sync::MultiprocessorSafeCell;
use wateros_base_config::fs::BOOTSTRAP_TMPFS_LIMIT_BYTES;

use crate::mount_ns::PerTaskMountNsRegistry;

/// 辅助挂载句柄：RW、RO、procfs/sysfs/securityfs 伪挂载或 bind 别名。
#[derive(Clone)]
pub(crate) enum AuxMount {
    Rw(SharedRwFs),
    Ro(SharedFs),
    PseudoProc,
    PseudoSys,
    PseudoSecurity,
    Bind { source : String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// 本结构代码由AI完成
pub enum MountPropagation {
    /// 挂载事件不向其它挂载点传播（默认）。
    Private,
    /// 挂载/卸载事件向共享组内其它挂载点传播。
    Shared,
    /// 接收 master 传播，但不向 peer 回传。
    Slave,
    /// 不参与 bind 与传播。
    Unbindable,
}

impl Default for MountPropagation {
    // 本方法代码由AI完成
    fn default() -> Self { Self::Private }
}

#[derive(Clone)]
// 本结构代码由AI完成
struct MountEntry {
    mount_point : String,
    fs : AuxMount,
    identity : MountIdentity,
    readonly : bool,
    fstype : &'static str,
    /// Linux `MS_*` 传播类型；bind 挂载时继承或显式设置。
    propagation : MountPropagation,
}

/// 单个挂载命名空间内的辅助卷表。
#[derive(Default)]
pub(crate) struct MountNamespace {
    entries : Vec<MountEntry>,
}

impl Clone for MountNamespace {
    // 本方法代码由AI完成
    fn clone(&self) -> Self { Self { entries : self.entries.clone() } }
}

// 本变量代码由AI完成
static DEVICE_IDS : Mutex<Vec<(String, u32)>> = Mutex::new(Vec::new());
// 本变量代码由AI完成
static NEXT_DEVICE_MINOR : AtomicU64 = AtomicU64::new(1);
// 本变量代码由AI完成
static NEXT_MOUNT_ID : AtomicU64 = AtomicU64::new(2);

/// 内核 bring-up / 无当前任务时的挂载表（`procfs` 等在 spawn 用户任务前挂载）。
// 本变量代码由AI完成
static BOOTSTRAP_MOUNT_NS : Mutex<Option<Arc<MountNamespace>>> = Mutex::new(None);

// 本方法代码由AI完成
pub(crate) fn bootstrap_mount_namespace_snapshot() -> Arc<MountNamespace> {
    BOOTSTRAP_MOUNT_NS.lock()
                      .get_or_insert_with(|| Arc::new(MountNamespace::default()))
                      .clone()
}

fn with_bootstrap_namespace<R>(f : impl FnOnce(&mut MountNamespace) -> VfsResult<R>)
                                -> VfsResult<R> {
    let mut slot = BOOTSTRAP_MOUNT_NS.lock();
    let namespace = slot.get_or_insert_with(|| Arc::new(MountNamespace::default()));
    f(Arc::make_mut(namespace))
}

// 本变量代码由AI完成
static mut MOUNT_NS_REGISTRY : MaybeUninit<MultiprocessorSafeCell<PerTaskMountNsRegistry>> =
    MaybeUninit::uninit();
// 本变量代码由AI完成
static MOUNT_NS_REGISTRY_READY : AtomicUsize = AtomicUsize::new(0);

// 本方法代码由AI完成
fn registry() -> &'static MultiprocessorSafeCell<PerTaskMountNsRegistry> {
    if MOUNT_NS_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            MOUNT_NS_REGISTRY.write(MultiprocessorSafeCell::new(PerTaskMountNsRegistry::new()));
        }
        MOUNT_NS_REGISTRY_READY.store(1, Ordering::Release);
    }
    unsafe { &*MOUNT_NS_REGISTRY.as_ptr() }
}

pub fn init_task_mount_ns(task_id : task::TaskId) {
    registry().exclusive_access()
              .init_task_mount_ns(task_id);
}

// 本方法代码由AI完成
pub fn copy_mount_ns_from_parent(child : task::TaskId, parent : task::TaskId) {
    registry().exclusive_access()
              .copy_mount_ns_from_parent(child, parent);
}

// 本方法代码由AI完成
pub fn share_mount_ns_from_parent(child : task::TaskId, parent : task::TaskId) {
    registry().exclusive_access()
              .share_mount_ns_from_parent(child, parent);
}

pub fn unshare_mount_ns(task_id : task::TaskId) {
    registry().exclusive_access()
              .unshare_mount_ns(task_id);
}

pub fn drop_task_mount_ns(task_id : task::TaskId) {
    registry().exclusive_access()
              .drop_task(task_id);
}

// 本方法代码由AI完成
fn with_current_namespace<R>(f : impl FnOnce(&mut MountNamespace) -> VfsResult<R>) -> VfsResult<R> {
    if let Some(task_id) = task::current_task_id() {
        let mut reg = registry().exclusive_access();
        f(reg.namespace_for_mut(task_id))
    } else {
        with_bootstrap_namespace(f)
    }
}

// 本方法代码由AI完成
#[allow(dead_code)]
fn namespace_for_route(task_id : task::TaskId) -> Option<Arc<MountNamespace>> {
    registry().exclusive_access()
              .namespace_for(task_id)
              .cloned()
}

/// 克隆当前应使用的挂载表快照（单次加锁，避免在 `with_current_namespace` 内重入）。
// 本方法代码由AI完成
fn mount_namespace_snapshot() -> Arc<MountNamespace> {
    if let Some(task_id) = task::current_task_id() {
        {
            let reg = registry().exclusive_access();
            if let Some(ns) = reg.namespace_for(task_id) {
                return ns.clone();
            }
        }
        {
            let mut reg = registry().exclusive_access();
            reg.init_task_mount_ns(task_id);
            if let Some(ns) = reg.namespace_for(task_id) {
                return ns.clone();
            }
        }
    }
    bootstrap_mount_namespace_snapshot()
}

/// 在已持有的 `ns` 上校验挂载点目录，不调用 [`resolve_route`]（避免 mount 表锁重入）。
// 本方法代码由AI完成
fn assert_mount_point_directory_in(ns : &MountNamespace, path : &str) -> VfsResult<()> {
    match resolve_material_route(ns, path)? {
        FsRoute::PseudoProc { .. } | FsRoute::PseudoSys { .. } | FsRoute::PseudoSecurity { .. } => {
            Err(VfsError::NotAFile)
        }
        FsRoute::Root { abs, .. } => {
            let meta = super::root_rw()?.lock()
                                        .metadata(abs.as_str())
                                        .map_err(super::map_fs_err)?;
            if meta.node_type != FsNodeType::Directory {
                return Err(VfsError::NotAFile);
            }
            Ok(())
        }
        FsRoute::AuxRw { fs, rel, .. } => {
            let meta = fs.lock()
                         .metadata(rel.as_str())
                         .map_err(super::map_fs_err)?;
            if meta.node_type != FsNodeType::Directory {
                return Err(VfsError::NotAFile);
            }
            Ok(())
        }
        FsRoute::AuxRo { fs, rel, .. } => {
            let meta = fs.lock()
                         .metadata(rel.as_str())
                         .map_err(super::map_fs_err)?;
            if meta.node_type != FsNodeType::Directory {
                return Err(VfsError::NotAFile);
            }
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MountIdentity {
    pub device_major : u32,
    pub device_minor : u32,
    pub mount_id : u64,
}

// 本方法代码由AI完成
fn device_minor_for(key : &str) -> u32 {
    let mut devices = DEVICE_IDS.lock();
    if let Some((_, minor)) = devices.iter()
                                     .find(|(known, _)| known == key)
    {
        return *minor;
    }
    let minor =
        u32::try_from(NEXT_DEVICE_MINOR.fetch_add(1, Ordering::Relaxed)).expect("VFS device id \
                                                                                 exhausted");
    devices.push((String::from(key), minor));
    minor
}

// 本方法代码由AI完成
fn new_mount_identity(device_key : &str) -> MountIdentity {
    MountIdentity { device_major : 0,
                    device_minor : device_minor_for(device_key),
                    mount_id : NEXT_MOUNT_ID.fetch_add(1, Ordering::Relaxed) }
}

// 本方法代码由AI完成
pub(crate) fn root_identity() -> MountIdentity {
    let device = fs::rootfs::active_impl::current_root_device_path().unwrap_or_else(|| {
                                                                        String::from("/dev/root")
                                                                    });
    MountIdentity { device_major : 0,
                    device_minor : device_minor_for(device.as_str()),
                    mount_id : 1 }
}

/// 路径路由：根卷相对路径，或辅助卷 + 卷内相对路径，或 procfs 伪挂载。
pub(crate) enum FsRoute {
    Root {
        abs : String,
        identity : MountIdentity,
    },
    AuxRw {
        fs : SharedRwFs,
        rel : String,
        identity : MountIdentity,
        readonly : bool,
    },
    AuxRo {
        fs : SharedFs,
        rel : String,
        identity : MountIdentity,
    },
    PseudoProc {
        rel : String,
        identity : MountIdentity,
    },
    PseudoSys {
        rel : String,
        identity : MountIdentity,
    },
    PseudoSecurity {
        rel : String,
        identity : MountIdentity,
    },
}

// 本方法代码由AI完成
fn rel_under_mount(full : &str, mount_point : &str) -> String {
    if full == mount_point {
        return String::from("/");
    }
    let rest = full.strip_prefix(mount_point)
                   .unwrap_or(full);
    if rest.is_empty() {
        String::from("/")
    } else if rest.starts_with('/') {
        String::from(rest)
    } else {
        alloc::format!("/{}", rest)
    }
}

// 本方法代码由AI完成
fn join_mount_path(mount : &str, rel : &str) -> String {
    if rel == "/" {
        return String::from(mount);
    }
    let mount = mount.trim_end_matches('/');
    alloc::format!("{mount}{rel}")
}

impl AuxMount {
    // 本方法代码由AI完成
    #[allow(dead_code)]
    fn clone_mount(&self) -> Self {
        match self {
            Self::Rw(fs) => Self::Rw(fs.clone()),
            Self::Ro(fs) => Self::Ro(fs.clone()),
            Self::PseudoProc => Self::PseudoProc,
            Self::PseudoSys => Self::PseudoSys,
            Self::PseudoSecurity => Self::PseudoSecurity,
            Self::Bind { source } => Self::Bind { source : source.clone() },
        }
    }
}

impl MountNamespace {
    // 本方法代码由AI完成
    #[allow(dead_code)]
    pub fn new() -> Self { Self { entries : Vec::new() } }

    // 本方法代码由AI完成
    fn longest_match(&self, abs : &str) -> Option<(usize, &MountEntry)> {
        let mut best : Option<(usize, &MountEntry)> = None;
        for ent in self.entries.iter() {
            let mp = ent.mount_point
                        .as_str();
            let matches = abs == mp ||
                          abs.starts_with(mp) &&
                          abs.as_bytes()
                             .get(mp.len()) ==
                          Some(&b'/');
            if !matches {
                continue;
            }
            let len = mp.len();
            if best.as_ref()
                   .map(|(l, _)| len > *l)
                   .unwrap_or(true)
            {
                best = Some((len, ent));
            }
        }
        best
    }

    // 本方法代码由AI完成
    fn exact_mount(&self, mount_point : &str) -> Option<&MountEntry> {
        self.entries
            .iter()
            .find(|e| e.mount_point == mount_point)
    }

    // 本方法代码由AI完成
    fn exact_mount_mut(&mut self, mount_point : &str) -> Option<&mut MountEntry> {
        self.entries
            .iter_mut()
            .find(|e| e.mount_point == mount_point)
    }

    // 本方法代码由AI完成
    fn is_mount_point(&self, abs : &str) -> bool {
        self.exact_mount(abs)
            .is_some()
    }

    // 本方法代码由AI完成
    fn is_under_mount(&self, abs : &str, mount_point : &str) -> bool {
        abs == mount_point ||
        abs.starts_with(mount_point) &&
        abs.as_bytes()
           .get(mount_point.len()) ==
        Some(&b'/')
    }

    // 本方法代码由AI完成
    fn propagation_at(&self, abs : &str) -> MountPropagation {
        self.longest_match(abs)
            .map(|(_, ent)| ent.propagation)
            .unwrap_or(MountPropagation::Private)
    }

    // 本方法代码由AI完成
    fn bind_forbidden(&self, source : &str) -> bool {
        matches!(self.propagation_at(source),
                 MountPropagation::Unbindable)
    }
}

// 本变量代码由AI完成
const BIND_CHAIN_LIMIT : usize = 32;

// 本方法代码由AI完成
fn resolve_material_route(ns : &MountNamespace, abs : &str) -> VfsResult<FsRoute> {
    let abs = String::from(normalize_absolute_path(abs)?.as_str());
    let mut current = abs;
    for _ in 0..BIND_CHAIN_LIMIT {
        let Some((_, ent)) = ns.longest_match(current.as_str()) else {
            return Ok(FsRoute::Root { abs : current,
                                      identity : root_identity() });
        };
        let rel = rel_under_mount(current.as_str(),
                                  ent.mount_point
                                     .as_str());
        match &ent.fs {
            AuxMount::Bind { source } => {
                current = join_mount_path(source.as_str(), rel.as_str());
                continue;
            }
            AuxMount::Rw(fs) => {
                return Ok(FsRoute::AuxRw { fs : fs.clone(),
                                           rel,
                                           identity : ent.identity,
                                           readonly : ent.readonly });
            }
            AuxMount::Ro(fs) => {
                return Ok(FsRoute::AuxRo { fs : fs.clone(),
                                           rel,
                                           identity : ent.identity });
            }
            AuxMount::PseudoProc => {
                return Ok(FsRoute::PseudoProc { rel,
                                                identity : ent.identity });
            }
            AuxMount::PseudoSys => {
                return Ok(FsRoute::PseudoSys { rel,
                                               identity : ent.identity });
            }
            AuxMount::PseudoSecurity => {
                return Ok(FsRoute::PseudoSecurity { rel,
                                                    identity : ent.identity });
            }
        }
    }
    Err(VfsError::InvalidPath)
}

// 本方法代码由AI完成
fn bump_mount_generation_after_cache_flush() {
    if let Err(e) = super::reset_file_page_cache() {
        log::warn!("[vfs-bridge] page cache flush before mount_gen bump failed: {:?}",
                   e);
    }
    fs::rootfs::active_impl::bump_mount_generation();
}

// 本方法代码由AI完成
fn mount_aux_common(ns : &mut MountNamespace,
                    mount_point : &str,
                    fs : AuxMount,
                    device_key : &str,
                    fstype : &'static str,
                    readonly : bool)
                    -> VfsResult<()> {
    let mp = String::from(normalize_absolute_path(mount_point)?.as_str());
    if mp == "/" {
        return Err(VfsError::InvalidPath);
    }
    match assert_mount_point_directory_in(ns, mp.as_str()) {
        Ok(()) => {}
        Err(e) => return Err(e),
    }
    if ns.entries
         .iter()
         .any(|e| e.mount_point == mp)
    {
        return Err(VfsError::Exists);
    }
    ns.entries
      .push(MountEntry { mount_point : mp,
                         fs,
                         identity : new_mount_identity(device_key),
                         readonly,
                         fstype,
                         propagation : MountPropagation::Private });
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn resolve_route(path : &str) -> VfsResult<FsRoute> {
    let ns = mount_namespace_snapshot();
    resolve_material_route(&ns, path)
}

/// 写路径、带 `O_CREAT`/`O_WRONLY` 的 open 等须先调用；RO / procfs 返回 [`VfsError::ReadOnlyFs`]。
// 本方法代码由AI完成
pub fn assert_path_writable(path : &str) -> VfsResult<()> {
    match resolve_route(path)? {
        FsRoute::AuxRw { readonly: true, .. } |
        FsRoute::AuxRo { .. } |
        FsRoute::PseudoProc { .. } |
        FsRoute::PseudoSys { .. } |
        FsRoute::PseudoSecurity { .. } => Err(VfsError::ReadOnlyFs),
        _ => Ok(()),
    }
}

// 本方法代码由AI完成
pub(crate) fn mount_aux_at_rw(mount_point : &str,
                              fs : SharedRwFs,
                              device_key : &str)
                              -> VfsResult<()> {
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::Rw(fs),
                         device_key,
                         "ext4",
                         false)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn mount_aux_at_ro(mount_point : &str,
                              fs : SharedFs,
                              device_key : &str)
                              -> VfsResult<()> {
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::Ro(fs),
                         device_key,
                         "ext4",
                         true)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn mount_tmpfs_at(mount_point : &str) -> VfsResult<()> {
    mount_tmpfs_at_with_limit(mount_point, None)
}

// 本方法代码由AI完成
pub(crate) fn mount_tmpfs_at_with_limit(mount_point : &str,
                                        limit_bytes : Option<usize>)
                                        -> VfsResult<()> {
    let fs : SharedRwFs = fs::new_ramfs_rw(limit_bytes, 0o1777);
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::Rw(fs),
                         "tmpfs",
                         "tmpfs",
                         false)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

/// 在 bootstrap namespace 中挂载 tmpfs，供之后创建的任务继承。
pub(crate) fn mount_bootstrap_tmpfs_at(mount_point : &str) -> VfsResult<()> {
    let fs : SharedRwFs = fs::new_ramfs_rw(Some(BOOTSTRAP_TMPFS_LIMIT_BYTES),
                                           0o1777);
    {
        with_bootstrap_namespace(|ns| {
            mount_aux_common(ns,
                             mount_point,
                             AuxMount::Rw(fs),
                             "tmpfs",
                             "tmpfs",
                             false)
        })?;
    }
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn mount_cgroup_at(mount_point : &str, v2 : bool, options : &str) -> VfsResult<()> {
    let tmp = super::tmpfs::TmpFs::new_cgroup(v2, options).map_err(super::map_fs_err)?;
    let fs : SharedRwFs = Arc::new(Mutex::new(LocalRwFs::new(Box::new(tmp))));
    let fstype = if v2 { "cgroup2" } else { "cgroup" };
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::Rw(fs),
                         "cgroup",
                         fstype,
                         false)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn mount_securityfs_at(mount_point : &str) -> VfsResult<()> {
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::PseudoSecurity,
                         "securityfs",
                         "securityfs",
                         true)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn mount_bind_at(source : &str, target : &str, recursive : bool) -> VfsResult<()> {
    let source = String::from(normalize_absolute_path(source)?.as_str());
    let target = String::from(normalize_absolute_path(target)?.as_str());
    if target == "/" {
        return Err(VfsError::InvalidPath);
    }
    with_current_namespace(|ns| {
        if ns.bind_forbidden(source.as_str()) {
            return Err(VfsError::InvalidPath);
        }
        assert_mount_point_directory_in(ns, target.as_str())?;
        if ns.is_mount_point(target.as_str()) {
            return Err(VfsError::Exists);
        }
        if recursive {
            recursive_bind(ns, source.as_str(), target.as_str())?;
        } else {
            ns.entries
              .push(MountEntry { mount_point : target,
                                 fs : AuxMount::Bind { source },
                                 identity : new_mount_identity("bind"),
                                 readonly : false,
                                 fstype : "bind",
                                 propagation : MountPropagation::Private });
        }
        Ok(())
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
fn recursive_bind(ns : &mut MountNamespace,
                  source_root : &str,
                  target_root : &str)
                  -> VfsResult<()> {
    let mut pairs : Vec<(String, String)> = Vec::new();
    pairs.push((String::from(source_root), String::from(target_root)));
    let mut mounts : Vec<String> = ns.entries
                                     .iter()
                                     .filter_map(|e| {
                                         if ns.is_under_mount(e.mount_point
                                                               .as_str(),
                                                              source_root) &&
                                            e.mount_point != source_root
                                         {
                                             Some(e.mount_point
                                                   .clone())
                                         } else {
                                             None
                                         }
                                     })
                                     .collect();
    mounts.sort_by_key(|p| p.len());
    for mp in mounts {
        let rel = rel_under_mount(mp.as_str(), source_root);
        let dst = join_mount_path(target_root, rel.as_str());
        pairs.push((mp, dst));
    }
    for (src, dst) in pairs {
        if ns.is_mount_point(dst.as_str()) {
            continue;
        }
        assert_mount_point_directory_in(ns, dst.as_str())?;
        ns.entries
          .push(MountEntry { mount_point : dst,
                             fs : AuxMount::Bind { source : src.clone() },
                             identity : new_mount_identity("bind"),
                             readonly : false,
                             fstype : "bind",
                             propagation : MountPropagation::Private });
    }
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn set_mount_propagation(mount_point : &str,
                                    propagation : MountPropagation,
                                    recursive : bool)
                                    -> VfsResult<()> {
    let mp = String::from(normalize_absolute_path(mount_point)?.as_str());
    with_current_namespace(|ns| {
        if !ns.is_mount_point(mp.as_str()) {
            return Err(VfsError::NotFound);
        }
        let targets : Vec<String> = if recursive {
            ns.entries
              .iter()
              .filter_map(|e| {
                  if e.mount_point == mp ||
                     ns.is_under_mount(e.mount_point
                                        .as_str(),
                                       mp.as_str())
                  {
                      Some(e.mount_point
                            .clone())
                  } else {
                      None
                  }
              })
              .collect()
        } else {
            alloc::vec![mp.clone()]
        };
        for target in targets {
            if let Some(ent) = ns.exact_mount_mut(target.as_str()) {
                ent.propagation = propagation;
            }
        }
        Ok(())
    })
}

// 本方法代码由AI完成
pub(crate) fn move_mount_at(source : &str, target : &str) -> VfsResult<()> {
    let source = String::from(normalize_absolute_path(source)?.as_str());
    let target = String::from(normalize_absolute_path(target)?.as_str());
    if source == "/" || target == "/" {
        return Err(VfsError::InvalidPath);
    }
    with_current_namespace(|ns| {
        if !ns.is_mount_point(source.as_str()) {
            return Err(VfsError::NotFound);
        }
        if ns.is_under_mount(target.as_str(), source.as_str()) {
            return Err(VfsError::InvalidPath);
        }
        if ns.is_mount_point(target.as_str()) {
            return Err(VfsError::Exists);
        }
        assert_mount_point_directory_in(ns, target.as_str())?;
        let prefix = source.clone();
        let mut renames : Vec<(String, String)> = Vec::new();
        for ent in ns.entries.iter() {
            if ent.mount_point == prefix {
                renames.push((ent.mount_point
                                 .clone(),
                              target.clone()));
            } else if ent.mount_point
                         .starts_with(&prefix)
            {
                let rest = ent.mount_point
                              .strip_prefix(&prefix)
                              .unwrap_or("");
                renames.push((ent.mount_point
                                 .clone(),
                              join_mount_path(target.as_str(), rest)));
            }
        }
        for (old, new) in renames {
            if let Some(ent) = ns.exact_mount_mut(old.as_str()) {
                ent.mount_point = new;
            }
        }
        Ok(())
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub fn mount_statfs_magic(abs : &str) -> Option<isize> {
    let Ok(abs) = normalize_absolute_path(abs) else {
        return None;
    };
    // 与 resolve_route 使用同一套快照（含惰性初始化兜底）：per-task 命名空间
    // 可能尚未从 bootstrap 命名空间同步 /proc 等早期挂载，直接查 registry 会
    // 漏掉挂载条目，导致 statfs 回退到 ext4 魔数（systemd 据此误报 /proc
    // 未挂载）。
    let ns = mount_namespace_snapshot();
    let (_, ent) = ns.longest_match(abs.as_str())?;
    Some(match ent.fstype {
        "tmpfs" => 0x0102_1994,
        "cgroup" => 0x0027_E0EB,
        "cgroup2" => 0x6367_7270,
        "proc" => 0x9FA0,
        "sysfs" => 0x6265_6572,
        "securityfs" => 0x7363_6673,
        "bind" => mount_statfs_magic_for_path(&ns, abs.as_str()).unwrap_or(0xEF53),
        _ => 0xEF53,
    })
}

// 本方法代码由AI完成
fn mount_statfs_magic_for_path(ns : &MountNamespace, abs : &str) -> Option<isize> {
    let route = resolve_material_route(ns, abs).ok()?;
    let path = match route {
        FsRoute::Root { abs, .. } => abs,
        FsRoute::AuxRw { .. } | FsRoute::AuxRo { .. } => return Some(0xEF53),
        FsRoute::PseudoProc { .. } => return Some(0x9FA0),
        FsRoute::PseudoSys { .. } => return Some(0x6265_6572),
        FsRoute::PseudoSecurity { .. } => return Some(0x7363_6673),
    };
    mount_statfs_magic(path.as_str())
}

// 本方法代码由AI完成
pub(crate) fn remount_aux_readonly(mount_point : &str) -> VfsResult<()> {
    let mp = String::from(normalize_absolute_path(mount_point)?.as_str());
    with_current_namespace(|ns| {
        let ent = ns.exact_mount_mut(mp.as_str())
                    .ok_or(VfsError::NotFound)?;
        if !matches!(ent.fs, AuxMount::Rw(_)) {
            return Err(VfsError::InvalidPath);
        }
        ent.readonly = true;
        Ok(())
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub fn mount_aux_proc_at(mount_point : &str) -> VfsResult<()> {
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::PseudoProc,
                         "proc",
                         "proc",
                         true)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

/// 在当前挂载命名空间中挂载 sysfs。
pub fn mount_aux_sys_at(mount_point : &str) -> VfsResult<()> {
    with_current_namespace(|ns| {
        mount_aux_common(ns,
                         mount_point,
                         AuxMount::PseudoSys,
                         "sysfs",
                         "sysfs",
                         true)
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

/// 在 bootstrap namespace 中挂载 procfs，供之后创建的内核/用户任务继承。
pub fn mount_bootstrap_proc_at(mount_point : &str) -> VfsResult<()> {
    {
        with_bootstrap_namespace(|ns| {
            mount_aux_common(ns,
                             mount_point,
                             AuxMount::PseudoProc,
                             "proc",
                             "proc",
                             true)
        })?;
    }
    bump_mount_generation_after_cache_flush();
    Ok(())
}

/// 在 bootstrap namespace 中挂载 sysfs，供之后创建的任务继承。
pub fn mount_bootstrap_sys_at(mount_point : &str) -> VfsResult<()> {
    {
        with_bootstrap_namespace(|ns| {
            mount_aux_common(ns,
                             mount_point,
                             AuxMount::PseudoSys,
                             "sysfs",
                             "sysfs",
                             true)
        })?;
    }
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub fn is_proc_mounted_at(mount_point : &str) -> bool {
    let Ok(mp) = normalize_absolute_path(mount_point) else {
        return false;
    };
    if let Some(task_id) = task::current_task_id() {
        let reg = registry().exclusive_access();
        if let Some(ns) = reg.namespace_for(task_id) {
            return ns.entries.iter().any(|e| {
                e.mount_point == mp.as_str() && matches!(e.fs, AuxMount::PseudoProc)
            });
        }
    }
    let ns = bootstrap_mount_namespace_snapshot();
    ns.entries
      .iter()
      .any(|e| e.mount_point == mp.as_str() && matches!(e.fs, AuxMount::PseudoProc))
}

pub fn is_mount_point(mount_point : &str) -> bool {
    let Ok(mp) = normalize_absolute_path(mount_point) else {
        return false;
    };
    mount_namespace_snapshot().is_mount_point(mp.as_str())
}

// 本方法代码由AI完成
fn fstype_for(entry : &MountEntry) -> &'static str { entry.fstype }

// 本方法代码由AI完成
fn device_for(entry : &MountEntry) -> String {
    match entry.fs {
        AuxMount::PseudoProc => String::from("proc"),
        AuxMount::PseudoSys => String::from("sysfs"),
        AuxMount::PseudoSecurity => String::from("securityfs"),
        AuxMount::Bind { ref source } => source.clone(),
        AuxMount::Rw(_) | AuxMount::Ro(_) => entry.mount_point
                                                  .clone(),
    }
}

// 本方法代码由AI完成
fn root_mount_device() -> String {
    fs::devfs::active_impl::default_root_block_path().unwrap_or_else(|| String::from("/dev/root"))
}

// 本方法代码由AI完成
pub fn list_proc_mount_lines() -> Vec<ProcMountLine> {
    let mut out = Vec::new();
    if fs::rootfs::active_impl::root_rw_fs().is_some() {
        out.push(ProcMountLine { device : root_mount_device(),
                                 mount_point : String::from("/"),
                                 fstype : String::from("ext4"),
                                 readonly : false });
    }
    if let Some(task_id) = task::current_task_id() {
        let reg = registry().exclusive_access();
        if let Some(ns) = reg.namespace_for(task_id) {
            for ent in ns.entries.iter() {
                out.push(ProcMountLine { device : device_for(ent),
                                         mount_point : ent.mount_point
                                                          .clone(),
                                         fstype : String::from(fstype_for(ent)),
                                         readonly : ent.readonly });
            }
            return out;
        }
    }
    for ent in bootstrap_mount_namespace_snapshot().entries.iter()
    {
        out.push(ProcMountLine { device : device_for(ent),
                                 mount_point : ent.mount_point
                                                  .clone(),
                                 fstype : String::from(fstype_for(ent)),
                                 readonly : ent.readonly });
    }
    out
}

// 本方法代码由AI完成
pub(crate) fn unmount_aux_at(mount_point : &str, detach : bool) -> VfsResult<()> {
    let mp = String::from(normalize_absolute_path(mount_point)?.as_str());
    if mp == "/" {
        return Err(VfsError::InvalidPath);
    }
    let _ = detach;
    with_current_namespace(|ns| {
        let pos = ns.entries
                    .iter()
                    .position(|e| e.mount_point == mp)
                    .ok_or(VfsError::NotFound)?;
        ns.entries
          .remove(pos);
        Ok(())
    })?;
    bump_mount_generation_after_cache_flush();
    Ok(())
}

// 本方法代码由AI完成
pub fn mount_table_self_test() -> VfsResult<()> {
    let dev_a = new_mount_identity("/dev/__identity_test__");
    let dev_b = new_mount_identity("/dev/__identity_test__");
    assert_eq!(dev_a.device_major, dev_b.device_major);
    assert_eq!(dev_a.device_minor, dev_b.device_minor);
    assert_ne!(dev_a.mount_id, dev_b.mount_id);

    let task_id = task::TaskId::from(0xFEED_usize);
    init_task_mount_ns(task_id);
    let reg = registry().exclusive_access();
    let n_before = reg.namespace_for(task_id)
                      .map(|ns| ns.entries.len())
                      .unwrap_or(0);
    drop(reg);

    let root = super::root_rw()?;
    let mp = "/__bringup_mount_test__";
    {
        let mut reg = registry().exclusive_access();
        let ns = reg.namespace_for_mut(task_id);
        mount_aux_common(ns,
                         mp,
                         AuxMount::Rw(root.clone()),
                         "/dev/root-self-test",
                         "ext4",
                         false)?;
    }
    {
        let probe = alloc::format!("{mp}/x");
        let reg = registry().exclusive_access();
        let ns = reg.namespace_for(task_id)
                    .ok_or(VfsError::Io)?;
        let route = resolve_material_route(ns, probe.as_str())?;
        match route {
            FsRoute::AuxRw { rel, .. } if rel == "/x" => {}
            _ => return Err(VfsError::Io),
        }
    }
    unmount_aux_at(mp, false)?;
    let reg = registry().exclusive_access();
    if reg.namespace_for(task_id)
          .map(|ns| ns.entries.len())
          .unwrap_or(0) !=
       n_before
    {
        return Err(VfsError::Io);
    }
    Ok(())
}
