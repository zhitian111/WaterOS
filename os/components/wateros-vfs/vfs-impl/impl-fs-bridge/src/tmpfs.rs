//! 内存 tmpfs：供 LTP `needs_rofs` 等测例挂载可重载为只读的临时卷。

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
    },
    Dir {
        children: BTreeMap<String, TmpNode>,
        mode: u16,
        inode: u64,
    },
    Symlink {
        target: Vec<u8>,
        mode: u16,
        inode: u64,
    },
}

pub(crate) struct TmpFs {
    root: TmpNode,
    next_inode: u64,
    mounted: bool,
    cgroup_v2: Option<bool>,
}

impl TmpFs {
    pub(crate) fn new() -> Self {
        Self {
            root: TmpNode::Dir {
                children: BTreeMap::new(),
                mode: 0o40755,
                inode: 1,
            },
            next_inode: 2,
            mounted: true,
            cgroup_v2: None,
        }
    }

    pub(crate) fn new_cgroup(v2: bool) -> FsResult<Self> {
        let mut fs = Self::new();
        fs.cgroup_v2 = Some(v2);
        fs.seed_cgroup_controls("/")?;
        Ok(fs)
    }

    fn alloc_inode(&mut self) -> u64 {
        let n = self.next_inode;
        self.next_inode += 1;
        n
    }

    fn split_path(path: &str) -> FsResult<Vec<&str>> {
        let p = path.trim();
        let p = p.strip_prefix('/').unwrap_or(p);
        if p.is_empty() {
            return Ok(Vec::new());
        }
        Ok(p.split('/').filter(|s| !s.is_empty() && *s != ".").collect())
    }

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

    fn meta_of(node: &TmpNode) -> FsMetadata {
        match node {
            TmpNode::File { data, mode, inode } => FsMetadata {
                node_type: FsNodeType::File,
                size: data.len() as u64,
                mode: *mode,
                inode: *inode,
                nlink: 1,
            },
            TmpNode::Dir { mode, inode, .. } => FsMetadata {
                node_type: FsNodeType::Directory,
                size: 0,
                mode: *mode,
                inode: *inode,
                nlink: 2,
            },
            TmpNode::Symlink { target, mode, inode } => FsMetadata {
                node_type: FsNodeType::Symlink,
                size: target.len() as u64,
                mode: *mode,
                inode: *inode,
                nlink: 1,
            },
        }
    }

    fn remove_leaf(root: &mut TmpNode, parts: &[&str]) -> FsResult<TmpNode> {
        let (children, name) = Self::parent_dir_mut(root, parts)?;
        children.remove(name).ok_or(FsError::NotFound)
    }

    fn insert_leaf(root: &mut TmpNode, parts: &[&str], node: TmpNode) -> FsResult<()> {
        let (children, name) = Self::parent_dir_mut(root, parts)?;
        if children.contains_key(name) {
            return Err(FsError::Exists);
        }
        children.insert(String::from(name), node);
        Ok(())
    }

    fn cgroup_control_files(v2: bool) -> &'static [(&'static str, &'static [u8])] {
        if v2 {
            &[("cgroup.procs", b""),
              ("cgroup.subtree_control", b""),
              ("cgroup.controllers", b"memory cpu cpuset io pids freezer\n"),
              ("cgroup.type", b"domain\n")]
        } else {
            &[("tasks", b""), ("cgroup.procs", b""), ("notify_on_release", b"0\n")]
        }
    }

    fn seed_cgroup_controls(&mut self, dir_path: &str) -> FsResult<()> {
        let Some(v2) = self.cgroup_v2 else {
            return Ok(());
        };
        for (name, data) in Self::cgroup_control_files(v2) {
            let path = if dir_path == "/" {
                alloc::format!("/{name}")
            } else {
                alloc::format!("{}/{}", dir_path.trim_end_matches('/'), name)
            };
            self.write_regular_file(path.as_str(), data)?;
        }
        Ok(())
    }

    fn is_cgroup_control_name(v2: Option<bool>, name: &str) -> bool {
        v2.is_some_and(|v2| Self::cgroup_control_files(v2).iter().any(|(n, _)| *n == name))
    }
}

impl ReadWriteFs for TmpFs {
    fn mount_rw(&mut self, _device: SharedBlockDevice) -> FsResult<()> {
        self.mounted = true;
        Ok(())
    }

    fn is_mounted(&self) -> bool {
        self.mounted
    }

    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> FsResult<()> {
        self.write_regular_file(alloc::format!("/{name}").as_str(), data)
    }

    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let inode = self.alloc_inode();
        let node = TmpNode::File {
            data: data.to_vec(),
            mode: 0o100644,
            inode,
        };
        let _ = Self::remove_leaf(&mut self.root, &parts);
        Self::insert_leaf(&mut self.root, &parts, node)
    }

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
        };
        Self::insert_leaf(&mut self.root, &parts, node)
    }

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

    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
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
        };
        Self::insert_leaf(&mut self.root, &parts, node)?;
        self.seed_cgroup_controls(path)
    }

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

    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        let _ = (path, uid, gid);
        Ok(())
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> FsResult<()> {
        let old_parts = Self::split_path(old_path)?;
        let new_parts = Self::split_path(new_path)?;
        let node = Self::remove_leaf(&mut self.root, &old_parts)?;
        if Self::dir_ref(&self.root, &new_parts).is_ok() {
            let _ = Self::insert_leaf(&mut self.root, &old_parts, node);
            return Err(FsError::Exists);
        }
        Self::insert_leaf(&mut self.root, &new_parts, node)
    }

    fn exists(&self, path: &str) -> FsResult<bool> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Ok(true);
        }
        Ok(Self::dir_ref(&self.root, &parts).is_ok())
    }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Ok(Self::meta_of(&self.root));
        }
        Ok(Self::meta_of(Self::dir_ref(&self.root, &parts)?))
    }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let TmpNode::File { data, .. } = node else {
            return Err(FsError::NotAFile);
        };
        Ok(data.clone())
    }

    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::dir_ref(&self.root, &parts)?;
        let TmpNode::Symlink { target, .. } = node else {
            return Err(FsError::NotAFile);
        };
        Ok(target.clone())
    }

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
