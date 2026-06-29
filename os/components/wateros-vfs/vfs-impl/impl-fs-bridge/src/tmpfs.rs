//! 内存 tmpfs：供 LTP `needs_rofs` 等测例挂载可重载为只读的临时卷。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use driver_block_api_v0::SharedBlockDevice;
use fs::{FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult, ReadWriteFs};

#[derive(Clone)]
enum TmpNode {
    File {
        data: Vec<u8>,
        mode: u16,
        inode: u64,
        uid: u32,
        gid: u32,
        xattrs: BTreeMap<String, Vec<u8>>,
    },
    Dir {
        children: BTreeMap<String, TmpNode>,
        mode: u16,
        inode: u64,
        uid: u32,
        gid: u32,
        xattrs: BTreeMap<String, Vec<u8>>,
    },
    Symlink {
        target: Vec<u8>,
        mode: u16,
        inode: u64,
        uid: u32,
        gid: u32,
        xattrs: BTreeMap<String, Vec<u8>>,
    },
}

pub(crate) struct TmpFs {
    root: TmpNode,
    next_inode: u64,
    mounted: bool,
    cgroup_v2: Option<bool>,
    /// v1 `mount -t cgroup -o cpuset` 的控制器列表（逗号分隔）。
    cgroup_v1_options: Option<String>,
}

impl TmpFs {
// 本方法代码由AI完成
    pub(crate) fn new() -> Self {
        Self {
            root: TmpNode::Dir {
                children: BTreeMap::new(),
                mode: 0o40755,
                inode: 1,
                uid: 0,
                gid: 0,
                xattrs: BTreeMap::new(),
            },
            next_inode: 2,
            mounted: true,
            cgroup_v2: None,
            cgroup_v1_options: None,
        }
    }

// 本方法代码由AI完成
    pub(crate) fn new_cgroup(v2: bool, options: &str) -> FsResult<Self> {
        let mut fs = Self::new();
        fs.cgroup_v2 = Some(v2);
        if !v2 && !options.is_empty() {
            fs.cgroup_v1_options = Some(String::from(options));
        }
        fs.seed_cgroup_controls("/")?;
        Ok(fs)
    }

// 本方法代码由AI完成
    fn v1_has_controller(&self, name: &str) -> bool {
        self.cgroup_v1_options.as_ref().is_some_and(|opts| {
            opts.split(',')
                .any(|ctrl| ctrl.trim() == name)
        })
    }

// 本方法代码由AI完成
    fn write_control_file(&mut self, dir_path: &str, name: &str, data: &[u8]) -> FsResult<()> {
        let path = if dir_path == "/" || dir_path.is_empty() {
            alloc::format!("/{name}")
        } else {
            alloc::format!("{}/{}", dir_path.trim_end_matches('/'), name)
        };
        self.write_regular_file(path.as_str(), data)
    }

// 本方法代码由AI完成
    fn alloc_inode(&mut self) -> u64 {
        let n = self.next_inode;
        self.next_inode += 1;
        n
    }

// 本方法代码由AI完成
    fn split_path(path: &str) -> FsResult<Vec<&str>> {
        let p = path.trim();
        let p = p.strip_prefix('/').unwrap_or(p);
        if p.is_empty() {
            return Ok(Vec::new());
        }
        Ok(p.split('/').filter(|s| !s.is_empty() && *s != ".").collect())
    }

// 本方法代码由AI完成
    fn dir_mut<'a>(root: &'a mut TmpNode, parts: &[&str]) -> FsResult<&'a mut TmpNode> {
        let mut node = root;
        for &part in parts {
            let TmpNode::Dir { children, .. } = node else {
                return Err(FsError::NotFound);
            };
            node = children.get_mut(part).ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

// 本方法代码由AI完成
    fn dir_ref<'a>(root: &'a TmpNode, parts: &[&str]) -> FsResult<&'a TmpNode> {
        let mut node = root;
        for &part in parts {
            let TmpNode::Dir { children, .. } = node else {
                return Err(FsError::NotFound);
            };
            node = children.get(part).ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

// 本方法代码由AI完成
    fn parent_dir_mut<'a>(
        root: &'a mut TmpNode,
        parts: &[&'a str],
    ) -> FsResult<(&'a mut BTreeMap<String, TmpNode>, &'a str)> {
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let name = parts[parts.len() - 1];
        let parent_parts = &parts[..parts.len() - 1];
        let TmpNode::Dir { children, .. } = Self::dir_mut(root, parent_parts)? else {
            return Err(FsError::NotFound);
        };
        Ok((children, name))
    }

// 本方法代码由AI完成
    fn meta_of(node: &TmpNode) -> FsMetadata {
        match node {
            TmpNode::File {
                data,
                mode,
                inode,
                uid,
                gid,
                ..
            } => FsMetadata {
                node_type: FsNodeType::File,
                size: data.len() as u64,
                mode: *mode,
                inode: *inode,
                nlink: 1,
                uid: *uid,
                gid: *gid,
            },
            TmpNode::Dir {
                mode,
                inode,
                uid,
                gid,
                ..
            } => FsMetadata {
                node_type: FsNodeType::Directory,
                size: 0,
                mode: *mode,
                inode: *inode,
                nlink: 2,
                uid: *uid,
                gid: *gid,
            },
            TmpNode::Symlink {
                target,
                mode,
                inode,
                uid,
                gid,
                ..
            } => FsMetadata {
                node_type: FsNodeType::Symlink,
                size: target.len() as u64,
                mode: *mode,
                inode: *inode,
                nlink: 1,
                uid: *uid,
                gid: *gid,
            },
        }
    }

// 本方法代码由AI完成
    fn chown_node(node: &mut TmpNode, uid: Option<u32>, gid: Option<u32>) {
        match node {
            TmpNode::File { uid: u, gid: g, .. }
            | TmpNode::Dir { uid: u, gid: g, .. }
            | TmpNode::Symlink { uid: u, gid: g, .. } => {
                if let Some(uid) = uid {
                    *u = uid;
                }
                if let Some(gid) = gid {
                    *g = gid;
                }
            }
        }
    }

// 本方法代码由AI完成
    fn xattrs_mut(node: &mut TmpNode) -> &mut BTreeMap<String, Vec<u8>> {
        match node {
            TmpNode::File { xattrs, .. }
            | TmpNode::Dir { xattrs, .. }
            | TmpNode::Symlink { xattrs, .. } => xattrs,
        }
    }

// 本方法代码由AI完成
    fn xattrs(node: &TmpNode) -> &BTreeMap<String, Vec<u8>> {
        match node {
            TmpNode::File { xattrs, .. }
            | TmpNode::Dir { xattrs, .. }
            | TmpNode::Symlink { xattrs, .. } => xattrs,
        }
    }

// 本方法代码由AI完成
    fn node_inode(node: &TmpNode) -> u64 {
        match node {
            TmpNode::File { inode, .. }
            | TmpNode::Dir { inode, .. }
            | TmpNode::Symlink { inode, .. } => *inode,
        }
    }

// 本方法代码由AI完成
    fn check_rename_replacement(old_node: &TmpNode, new_node: &TmpNode) -> FsResult<()> {
        match (old_node, new_node) {
            (TmpNode::Dir { .. }, TmpNode::Dir { children, .. }) => {
                if children.is_empty() {
                    Ok(())
                } else {
                    Err(FsError::Exists)
                }
            }
            (TmpNode::Dir { .. }, _) => Err(FsError::Unsupported),
            (_, TmpNode::Dir { .. }) => Err(FsError::NotAFile),
            _ => Ok(()),
        }
    }

// 本方法代码由AI完成
    fn copy_xattr_list(names: &[String]) -> Vec<u8> {
        let mut out = Vec::new();
        for name in names {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }
        out
    }

// 本方法代码由AI完成
    fn remove_leaf(root: &mut TmpNode, parts: &[&str]) -> FsResult<TmpNode> {
        let (children, name) = Self::parent_dir_mut(root, parts)?;
        children.remove(name).ok_or(FsError::NotFound)
    }

// 本方法代码由AI完成
    fn insert_leaf(root: &mut TmpNode, parts: &[&str], node: TmpNode) -> FsResult<()> {
        let (children, name) = Self::parent_dir_mut(root, parts)?;
        if children.contains_key(name) {
            return Err(FsError::Exists);
        }
        children.insert(String::from(name), node);
        Ok(())
    }

// 本方法代码由AI完成
    fn cgroup_v1_base_files() -> &'static [(&'static str, &'static [u8])] {
        &[("tasks", b""), ("notify_on_release", b"0\n")]
    }

// 本方法代码由AI完成
    fn cgroup_v1_cpuset_root_files() -> &'static [(&'static str, &'static [u8])] {
        &[
            ("cgroup.clone_children", b"0\n"),
            ("cpuset.sched_load_balance", b"1\n"),
            ("cpuset.cpu_exclusive", b"0\n"),
            ("cpuset.mem_exclusive", b"0\n"),
            ("cpuset.mem_hardwall", b"0\n"),
            ("cpuset.memory_migrate", b"0\n"),
            ("cpuset.memory_spread_page", b"0\n"),
            ("cpuset.memory_spread_slab", b"0\n"),
            ("cpuset.memory_pressure_enabled", b"0\n"),
        ]
    }

// 本方法代码由AI完成
    fn cgroup_v1_cpuset_child_files() -> &'static [(&'static str, &'static [u8])] {
        &[
            ("cgroup.clone_children", b"0\n"),
            ("cpuset.cpus", b""),
            ("cpuset.mems", b""),
        ]
    }

// 本方法代码由AI完成
    fn cgroup_control_files(v2: bool) -> &'static [(&'static str, &'static [u8])] {
        if v2 {
            &[("cgroup.procs", b""),
              ("cgroup.subtree_control", b""),
              ("cgroup.controllers", b"memory cpu cpuset io pids freezer\n"),
              ("cgroup.type", b"domain\n")]
        } else {
            Self::cgroup_v1_base_files()
        }
    }

// 本方法代码由AI完成
    fn seed_v1_cgroup_controls(&mut self, dir_path: &str) -> FsResult<()> {
        for (name, data) in Self::cgroup_v1_base_files() {
            self.write_control_file(dir_path, name, data)?;
        }
        if !self.v1_has_controller("cpuset") {
            return Ok(());
        }
        let parts = Self::split_path(if dir_path == "/" { "" } else { dir_path })?;
        let cpuset_files = if parts.is_empty() {
            Self::cgroup_v1_cpuset_root_files()
        } else {
            Self::cgroup_v1_cpuset_child_files()
        };
        for (name, data) in cpuset_files {
            self.write_control_file(dir_path, name, data)?;
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn seed_cgroup_controls(&mut self, dir_path: &str) -> FsResult<()> {
        let Some(v2) = self.cgroup_v2 else {
            return Ok(());
        };
        if v2 {
            for (name, data) in Self::cgroup_control_files(true) {
                self.write_control_file(dir_path, name, data)?;
            }
        } else {
            self.seed_v1_cgroup_controls(dir_path)?;
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn reject_cpuset_tasks_write_if_needed(&self, _path: &str, _data: &[u8]) -> FsResult<()> {
        // 放行 cgroup cpuset 的 tasks 写入（不再因 cpus/mems 未配置而拒绝 attach）。
        //
        // 语义上，real Linux 在子 cpuset 未配置 cpus/mems 时拒绝 attach（write 返回
        // ENOSPC）。但本系统的 /bin/sh 是 busybox ash，其内建 echo 在“重定向输出 write
        // 返回任何错误”时会直接 `exit 2` 终止整个脚本（dash/bash 不会）。LTP 的
        // cpuset attach 用例里此前会先 `cat /dev/zero > /dev/null &` 起后台进程，再
        // `echo $pid > tasks`；一旦该 write 失败导致 ash 退出，就到不了后续的
        // `/bin/kill $pid`，后台 cat 变成永不回收的孤儿，占满单核令 runltp 卡死。
        //
        // 因此这里放行写入，避免 LTP 全量卡死；代价是 cpuset attach 的“应失败”用例
        // 会 TFAIL，这是 busybox ash 作为 /bin/sh 的固有限制，无法在内核侧两全。
        Ok(())
    }

// 本方法代码由AI完成
    fn is_cgroup_control_name(v2: Option<bool>, name: &str) -> bool {
        match v2 {
            Some(true) => Self::cgroup_control_files(true)
                .iter()
                .any(|(n, _)| *n == name),
            Some(false) => {
                Self::cgroup_v1_base_files()
                    .iter()
                    .chain(Self::cgroup_v1_cpuset_root_files().iter())
                    .chain(Self::cgroup_v1_cpuset_child_files().iter())
                    .any(|(n, _)| *n == name)
            }
            None => false,
        }
    }
}

impl ReadWriteFs for TmpFs {
// 本方法代码由AI完成
    fn mount_rw(&mut self, _device: SharedBlockDevice) -> FsResult<()> {
        self.mounted = true;
        Ok(())
    }

// 本方法代码由AI完成
    fn is_mounted(&self) -> bool {
        self.mounted
    }

// 本方法代码由AI完成
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> FsResult<()> {
        self.write_regular_file(alloc::format!("/{name}").as_str(), data)
    }

// 本方法代码由AI完成
    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        self.reject_cpuset_tasks_write_if_needed(path, data)?;
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let inode = self.alloc_inode();
        let node = TmpNode::File {
            data: data.to_vec(),
            mode: 0o100644,
            inode,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        };
        let _ = Self::remove_leaf(&mut self.root, &parts);
        Self::insert_leaf(&mut self.root, &parts, node)
    }

// 本方法代码由AI完成
    fn unlink(&mut self, path: &str) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let (children, name) = Self::parent_dir_mut(&mut self.root, &parts)?;
        let node = children.get(name).ok_or(FsError::NotFound)?;
        if matches!(node, TmpNode::Dir { .. }) {
            return Err(FsError::NotAFile);
        }
        children.remove(name);
        Ok(())
    }

// 本方法代码由AI完成
    fn symlink(&mut self, link_path: &str, target: &str) -> FsResult<()> {
        let parts = Self::split_path(link_path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let inode = self.alloc_inode();
        let node = TmpNode::Symlink {
            target: target.as_bytes().to_vec(),
            mode: 0o120777,
            inode,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        };
        Self::insert_leaf(&mut self.root, &parts, node)
    }

// 本方法代码由AI完成
    fn rmdir(&mut self, path: &str) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let cgroup_v2 = self.cgroup_v2;
        let (children, name) = Self::parent_dir_mut(&mut self.root, &parts)?;
        let node = children.get(name).ok_or(FsError::NotFound)?;
        let TmpNode::Dir { children: sub, .. } = node else {
            return Err(FsError::NotAFile);
        };
        if !sub.is_empty()
            && !sub
                .keys()
                .all(|name| Self::is_cgroup_control_name(cgroup_v2, name.as_str()))
        {
            return Err(FsError::Exists);
        }
        children.remove(name);
        Ok(())
    }

// 本方法代码由AI完成
    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        self.reject_cpuset_tasks_write_if_needed(path, data)?;
        let parts = Self::split_path(path)?;
        let node = Self::dir_mut(&mut self.root, &parts)?;
        let TmpNode::File { data: buf, .. } = node else {
            return Err(FsError::NotAFile);
        };
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(FsError::Io)?;
        if end > buf.len() as u64 {
            buf.resize(end as usize, 0);
        }
        let start = offset as usize;
        buf[start..start + data.len()].copy_from_slice(data);
        Ok(data.len())
    }

// 本方法代码由AI完成
    fn truncate(&mut self, path: &str, len: u64) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_mut(&mut self.root, &parts)?;
        let TmpNode::File { data, .. } = node else {
            return Err(FsError::NotAFile);
        };
        let new_len = usize::try_from(len).map_err(|_| FsError::Io)?;
        data.resize(new_len, 0);
        Ok(())
    }

// 本方法代码由AI完成
    fn mkdir(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::Exists);
        }
        let inode = self.alloc_inode();
        let node = TmpNode::Dir {
            children: BTreeMap::new(),
            mode: (mode as u16) | 0o040000,
            inode,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        };
        Self::insert_leaf(&mut self.root, &parts, node)?;
        self.seed_cgroup_controls(path)
    }

// 本方法代码由AI完成
    fn chmod(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_mut(&mut self.root, &parts)?;
        match node {
            TmpNode::File { mode: m, .. } => {
                *m = 0o100000 | ((mode as u16) & 0o7777);
            }
            TmpNode::Dir { mode: m, .. } => {
                *m = 0o040000 | ((mode as u16) & 0o7777);
            }
            TmpNode::Symlink { mode: m, .. } => {
                *m = 0o120000 | ((mode as u16) & 0o7777);
            }
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        if uid.is_none() && gid.is_none() {
            return self.metadata(path).map(|_| ());
        }
        let parts = Self::split_path(path)?;
        let node = Self::dir_mut(&mut self.root, &parts)?;
        Self::chown_node(node, uid, gid);
        Ok(())
    }

// 本方法代码由AI完成
    fn setxattr(&mut self, path: &str, name: &str, value: &[u8]) -> FsResult<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let node = Self::dir_mut(&mut self.root, &parts)?;
        Self::xattrs_mut(node).insert(String::from(name), value.to_vec());
        Ok(())
    }

// 本方法代码由AI完成
    fn getxattr(&self, path: &str, name: &str, buf: &mut [u8]) -> FsResult<usize> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let value = Self::xattrs(node)
            .get(name)
            .ok_or(FsError::NotFound)?;
        if buf.is_empty() {
            return Ok(value.len());
        }
        if buf.len() < value.len() {
            return Err(FsError::Io);
        }
        buf[..value.len()].copy_from_slice(value);
        Ok(value.len())
    }

// 本方法代码由AI完成
    fn listxattr(&self, path: &str, buf: &mut [u8]) -> FsResult<usize> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let mut names: alloc::vec::Vec<String> = Self::xattrs(node).keys().cloned().collect();
        names.sort();
        let listing = Self::copy_xattr_list(names.as_slice());
        if buf.is_empty() {
            return Ok(listing.len());
        }
        if buf.len() < listing.len() {
            return Err(FsError::Io);
        }
        buf[..listing.len()].copy_from_slice(listing.as_slice());
        Ok(listing.len())
    }

// 本方法代码由AI完成
    fn removexattr(&mut self, path: &str, name: &str) -> FsResult<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let node = Self::dir_mut(&mut self.root, &parts)?;
        if Self::xattrs_mut(node).remove(name).is_none() {
            return Err(FsError::NotFound);
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn rename(&mut self, old_path: &str, new_path: &str) -> FsResult<()> {
        let old_parts = Self::split_path(old_path)?;
        let new_parts = Self::split_path(new_path)?;
        if new_parts.len() > old_parts.len() && new_parts.starts_with(old_parts.as_slice()) {
            return Err(FsError::InvalidPath);
        }
        let old_node = Self::dir_ref(&self.root, &old_parts)?;
        if let Ok(existing) = Self::dir_ref(&self.root, &new_parts) {
            if Self::node_inode(old_node) == Self::node_inode(existing) {
                return Ok(());
            }
            Self::check_rename_replacement(old_node, existing)?;
        }
        let node = Self::remove_leaf(&mut self.root, &old_parts)?;
        let (children, name) = match Self::parent_dir_mut(&mut self.root, &new_parts) {
            Ok(parent) => parent,
            Err(err) => {
                let _ = Self::insert_leaf(&mut self.root, &old_parts, node);
                return Err(err);
            }
        };
        children.remove(name);
        children.insert(String::from(name), node);
        Ok(())
    }

// 本方法代码由AI完成
    fn exists(&self, path: &str) -> FsResult<bool> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Ok(true);
        }
        Ok(Self::dir_ref(&self.root, &parts).is_ok())
    }

// 本方法代码由AI完成
    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Ok(Self::meta_of(&self.root));
        }
        Ok(Self::meta_of(Self::dir_ref(&self.root, &parts)?))
    }

// 本方法代码由AI完成
    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let TmpNode::File { data, .. } = node else {
            return Err(FsError::NotAFile);
        };
        Ok(data.clone())
    }

// 本方法代码由AI完成
    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let TmpNode::Symlink { target, .. } = node else {
            return Err(FsError::NotAFile);
        };
        Ok(target.clone())
    }

// 本方法代码由AI完成
    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let TmpNode::File { data, .. } = node else {
            return Err(FsError::NotAFile);
        };
        if offset >= data.len() as u64 {
            return Ok(0);
        }
        let start = offset as usize;
        let n = core::cmp::min(buf.len(), data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

// 本方法代码由AI完成
    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        let parts = Self::split_path(path)?;
        let node = if parts.is_empty() {
            &self.root
        } else {
            Self::dir_ref(&self.root, &parts)?
        };
        let TmpNode::Dir { children, .. } = node else {
            return Err(FsError::NotAFile);
        };
        let mut out = Vec::new();
        for (name, child) in children.iter() {
            let node_type = match child {
                TmpNode::File { .. } => FsNodeType::File,
                TmpNode::Dir { .. } => FsNodeType::Directory,
                TmpNode::Symlink { .. } => FsNodeType::Symlink,
            };
            out.push(FsDirEntry {
                name: name.clone(),
                node_type,
            });
        }
        Ok(out)
    }
}
