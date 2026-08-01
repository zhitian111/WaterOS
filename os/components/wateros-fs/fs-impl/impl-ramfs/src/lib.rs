#![no_std]

//! Heap-backed ramfs implementation.
//!
//! This crate owns the in-memory directory tree and file data. VFS policies such
//! as tmpfs mount points, root mode, and `size=` limits are selected by callers
//! when constructing an instance.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use api_v0::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsImpl, FsKind, FsMetadata, FsNodeType,
    FsResult, LocalRwFs, ReadWriteFs, SharedFs, SharedRwFs,
};
use driver_block_api_v0::SharedBlockDevice;
use spin::Mutex;

const RAMFS_PAGE_SIZE: usize = 4096;

#[derive(Clone, Default)]
struct SparseFile {
    len: u64,
    pages: BTreeMap<u64, Vec<u8>>,
}

impl SparseFile {
    fn allocated_bytes_for_data(data: &[u8]) -> FsResult<usize> {
        let pages = data.chunks(RAMFS_PAGE_SIZE)
                        .filter(|page| page.iter().any(|byte| *byte != 0))
                        .count();
        pages.checked_mul(RAMFS_PAGE_SIZE).ok_or(FsError::NoSpace)
    }

    fn from_bytes(data: &[u8]) -> FsResult<Self> {
        let mut file = Self::default();
        file.write_at(0, data)?;
        Ok(file)
    }

    fn allocated_bytes(&self) -> usize {
        self.pages.len().saturating_mul(RAMFS_PAGE_SIZE)
    }

    fn additional_bytes_for_write(&self, offset: u64, data: &[u8]) -> FsResult<usize> {
        let mut added_pages = 0usize;
        let mut done = 0usize;
        while done < data.len() {
            let pos = offset.checked_add(done as u64).ok_or(FsError::Io)?;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(data.len() - done);
            if !self.pages.contains_key(&page_idx) &&
               data[done..done + chunk].iter().any(|byte| *byte != 0)
            {
                added_pages = added_pages.checked_add(1).ok_or(FsError::NoSpace)?;
            }
            done += chunk;
        }
        added_pages.checked_mul(RAMFS_PAGE_SIZE).ok_or(FsError::NoSpace)
    }

    fn zero_page() -> FsResult<Vec<u8>> {
        let mut page = Vec::new();
        page.try_reserve_exact(RAMFS_PAGE_SIZE)
            .map_err(|_| FsError::NoSpace)?;
        page.resize(RAMFS_PAGE_SIZE, 0);
        Ok(page)
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset.checked_add(data.len() as u64).ok_or(FsError::Io)?;
        let mut done = 0usize;
        while done < data.len() {
            let pos = offset + done as u64;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(data.len() - done);
            let source = &data[done..done + chunk];
            if source.iter().all(|byte| *byte == 0) {
                let remove = if let Some(page) = self.pages.get_mut(&page_idx) {
                    page[page_off..page_off + chunk].fill(0);
                    page.iter().all(|byte| *byte == 0)
                } else {
                    false
                };
                if remove {
                    self.pages.remove(&page_idx);
                }
            } else {
                if !self.pages.contains_key(&page_idx) {
                    self.pages.insert(page_idx, Self::zero_page()?);
                }
                self.pages.get_mut(&page_idx)
                          .ok_or(FsError::Io)?[page_off..page_off + chunk]
                          .copy_from_slice(source);
            }
            done += chunk;
        }
        self.len = self.len.max(end);
        Ok(data.len())
    }

    fn truncate(&mut self, len: u64) {
        if len < self.len {
            let first_removed = len.saturating_add(RAMFS_PAGE_SIZE as u64 - 1) /
                                RAMFS_PAGE_SIZE as u64;
            let removed: Vec<u64> = self.pages.range(first_removed..)
                                              .map(|(&page_idx, _)| page_idx)
                                              .collect();
            for page_idx in removed {
                self.pages.remove(&page_idx);
            }
            let tail = (len % RAMFS_PAGE_SIZE as u64) as usize;
            if tail != 0 {
                let page_idx = len / RAMFS_PAGE_SIZE as u64;
                let remove = if let Some(page) = self.pages.get_mut(&page_idx) {
                    page[tail..].fill(0);
                    page.iter().all(|byte| *byte == 0)
                } else {
                    false
                };
                if remove {
                    self.pages.remove(&page_idx);
                }
            }
        }
        self.len = len;
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let available = usize::try_from((self.len - offset).min(buf.len() as u64))
            .map_err(|_| FsError::Io)?;
        buf[..available].fill(0);
        let mut done = 0usize;
        while done < available {
            let pos = offset + done as u64;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(available - done);
            if let Some(page) = self.pages.get(&page_idx) {
                buf[done..done + chunk]
                    .copy_from_slice(&page[page_off..page_off + chunk]);
            }
            done += chunk;
        }
        Ok(available)
    }

    fn materialize(&self) -> FsResult<Vec<u8>> {
        let len = usize::try_from(self.len).map_err(|_| FsError::NoSpace)?;
        let mut data = Vec::new();
        data.try_reserve_exact(len).map_err(|_| FsError::NoSpace)?;
        data.resize(len, 0);
        self.read_at(0, data.as_mut_slice())?;
        Ok(data)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    File,
    Dir,
    Symlink,
    Special,
}

#[derive(Clone)]
struct Node {
    kind: NodeKind,
    file_data: SparseFile,
    data: Vec<u8>,
    children: BTreeMap<String, Node>,
    mode: u16,
    inode: u64,
    nlink: u32,
    uid: u32,
    gid: u32,
    xattrs: BTreeMap<String, Vec<u8>>,
}

impl Node {
    fn dir(inode: u64, mode: u16) -> Self {
        Self {
            kind: NodeKind::Dir,
            file_data: SparseFile::default(),
            data: Vec::new(),
            children: BTreeMap::new(),
            mode: 0o040000 | (mode & 0o7777),
            inode,
            nlink: 2,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        }
    }

    fn file(inode: u64, data: &[u8]) -> FsResult<Self> {
        Ok(Self {
            kind: NodeKind::File,
            file_data: SparseFile::from_bytes(data)?,
            data: Vec::new(),
            children: BTreeMap::new(),
            mode: 0o100644,
            inode,
            nlink: 1,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        })
    }

    fn symlink(inode: u64, target: &str) -> Self {
        Self {
            kind: NodeKind::Symlink,
            file_data: SparseFile::default(),
            data: target.as_bytes().to_vec(),
            children: BTreeMap::new(),
            mode: 0o120777,
            inode,
            nlink: 1,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        }
    }

    fn special(inode: u64, mode: u32, _rdev: u32) -> Self {
        Self {
            kind: NodeKind::Special,
            file_data: SparseFile::default(),
            data: Vec::new(),
            children: BTreeMap::new(),
            mode: mode as u16,
            inode,
            nlink: 1,
            uid: 0,
            gid: 0,
            xattrs: BTreeMap::new(),
        }
    }

    fn accounted_bytes(&self) -> usize {
        let own = match self.kind {
            NodeKind::File => self.file_data.allocated_bytes(),
            NodeKind::Symlink => self.data.len(),
            NodeKind::Dir | NodeKind::Special => 0,
        };
        let xattrs = self.xattrs.values().map(Vec::len).sum::<usize>();
        own + xattrs + self.children.values().map(Node::accounted_bytes).sum::<usize>()
    }

    fn metadata(&self) -> FsMetadata {
        let node_type = match self.kind {
            NodeKind::File => FsNodeType::File,
            NodeKind::Dir => FsNodeType::Directory,
            NodeKind::Symlink => FsNodeType::Symlink,
            NodeKind::Special => FsNodeType::Special,
        };
        FsMetadata {
            node_type,
            size: match self.kind {
                NodeKind::File => self.file_data.len,
                NodeKind::Symlink => self.data.len() as u64,
                NodeKind::Dir | NodeKind::Special => 0,
            },
            mode: self.mode,
            inode: self.inode,
            nlink: self.nlink,
            uid: self.uid,
            gid: self.gid,
        }
    }
}

/// Heap-backed ramfs tree.
pub struct RamFs {
    root: Node,
    next_inode: u64,
    mounted: bool,
    limit_bytes: Option<usize>,
}

impl RamFs {
    /// Create an unlimited heap-backed ramfs with a `0755` root directory.
    pub fn new() -> Self { Self::with_options(None, 0o755) }

    /// Create a heap-backed ramfs with an accounted data-byte limit.
    pub fn with_limit(limit_bytes: usize) -> Self { Self::with_options(Some(limit_bytes), 0o755) }

    /// Create a heap-backed ramfs with explicit limit and root mode.
    pub fn with_options(limit_bytes: Option<usize>, root_mode: u16) -> Self {
        Self {
            root: Node::dir(1, root_mode),
            next_inode: 2,
            mounted: true,
            limit_bytes,
        }
    }

    /// Bytes currently charged to file contents, symlink targets, and xattr values.
    pub fn used_bytes(&self) -> usize { self.root.accounted_bytes() }

    /// Configured maximum charged bytes, if any.
    pub fn limit_bytes(&self) -> Option<usize> { self.limit_bytes }

    fn alloc_inode(&mut self) -> u64 {
        let inode = self.next_inode;
        self.next_inode += 1;
        inode
    }

    fn split_path(path: &str) -> FsResult<Vec<&str>> {
        let p = path.trim();
        let p = p.strip_prefix('/').unwrap_or(p);
        if p.is_empty() {
            return Ok(Vec::new());
        }
        Ok(p.split('/').filter(|s| !s.is_empty() && *s != ".").collect())
    }

    fn node_ref<'a>(root: &'a Node, parts: &[&str]) -> FsResult<&'a Node> {
        let mut node = root;
        for part in parts {
            if node.kind != NodeKind::Dir {
                return Err(FsError::NotFound);
            }
            node = node.children.get(*part).ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

    fn node_mut<'a>(root: &'a mut Node, parts: &[&str]) -> FsResult<&'a mut Node> {
        let mut node = root;
        for part in parts {
            if node.kind != NodeKind::Dir {
                return Err(FsError::NotFound);
            }
            node = node.children.get_mut(*part).ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

    fn parent_mut<'a>(
        root: &'a mut Node,
        parts: &[&'a str],
    ) -> FsResult<(&'a mut BTreeMap<String, Node>, &'a str)> {
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let name = parts[parts.len() - 1];
        let parent = Self::node_mut(root, &parts[..parts.len() - 1])?;
        if parent.kind != NodeKind::Dir {
            return Err(FsError::NotFound);
        }
        Ok((&mut parent.children, name))
    }

    fn ensure_capacity_after_replace(&self, old: Option<&Node>, new: &Node) -> FsResult<()> {
        let old_bytes = old.map(Node::accounted_bytes).unwrap_or(0);
        let used = self.used_bytes().saturating_sub(old_bytes);
        let next = used.checked_add(new.accounted_bytes()).ok_or(FsError::NoSpace)?;
        match self.limit_bytes {
            Some(limit) if next > limit => Err(FsError::NoSpace),
            _ => Ok(()),
        }
    }

    fn ensure_capacity_delta(&self, old_bytes: usize, new_bytes: usize) -> FsResult<()> {
        if new_bytes <= old_bytes {
            return Ok(());
        }
        let next = self
            .used_bytes()
            .checked_add(new_bytes - old_bytes)
            .ok_or(FsError::NoSpace)?;
        match self.limit_bytes {
            Some(limit) if next > limit => Err(FsError::NoSpace),
            _ => Ok(()),
        }
    }

    fn insert_new(&mut self, parts: &[&str], node: Node) -> FsResult<()> {
        self.ensure_capacity_after_replace(None, &node)?;
        let (children, name) = Self::parent_mut(&mut self.root, parts)?;
        if children.contains_key(name) {
            return Err(FsError::Exists);
        }
        children.insert(String::from(name), node);
        Ok(())
    }

    fn check_rename_replacement(old_node: &Node, new_node: &Node) -> FsResult<()> {
        match (old_node.kind, new_node.kind) {
            (NodeKind::Dir, NodeKind::Dir) if new_node.children.is_empty() => Ok(()),
            (NodeKind::Dir, NodeKind::Dir) => Err(FsError::Exists),
            (NodeKind::Dir, _) => Err(FsError::Unsupported),
            (_, NodeKind::Dir) => Err(FsError::NotAFile),
            _ => Ok(()),
        }
    }
}

impl Default for RamFs {
    fn default() -> Self { Self::new() }
}

impl ReadWriteFs for RamFs {
    fn mount_rw(&mut self, _device: SharedBlockDevice) -> FsResult<()> {
        self.mounted = true;
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.mounted }

    fn sync(&mut self) -> FsResult<()> {
        // Data in ramfs is already committed to its in-memory tree.
        Ok(())
    }

    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> FsResult<()> {
        self.write_regular_file(alloc::format!("/{name}").as_str(), data)
    }

    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        if matches!(
            Self::node_ref(&self.root, &parts).ok().map(|node| node.kind),
            Some(NodeKind::Dir)
        ) {
            return Err(FsError::NotAFile);
        }
        let old_bytes = Self::node_ref(&self.root, &parts)
                             .ok()
                             .map(Node::accounted_bytes)
                             .unwrap_or(0);
        let new_bytes = SparseFile::allocated_bytes_for_data(data)?;
        self.ensure_capacity_delta(old_bytes, new_bytes)?;
        let node = Node::file(self.alloc_inode(), data)?;
        let (children, name) = Self::parent_mut(&mut self.root, &parts)?;
        children.insert(String::from(name), node);
        Ok(())
    }

    fn unlink(&mut self, path: &str) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let (children, name) = Self::parent_mut(&mut self.root, &parts)?;
        let node = children.get(name).ok_or(FsError::NotFound)?;
        if node.kind == NodeKind::Dir {
            return Err(FsError::NotAFile);
        }
        children.remove(name);
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let (children, name) = Self::parent_mut(&mut self.root, &parts)?;
        let node = children.get(name).ok_or(FsError::NotFound)?;
        if node.kind != NodeKind::Dir {
            return Err(FsError::NotAFile);
        }
        if !node.children.is_empty() {
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
        let old_len = {
            let node = Self::node_ref(&self.root, &parts)?;
            if node.kind != NodeKind::File {
                return Err(FsError::NotAFile);
            }
            node.file_data.allocated_bytes()
        };
        let end = offset.checked_add(data.len() as u64).ok_or(FsError::Io)?;
        let added = Self::node_ref(&self.root, &parts)?
            .file_data
            .additional_bytes_for_write(offset, data)?;
        self.ensure_capacity_delta(old_len, old_len.checked_add(added).ok_or(FsError::NoSpace)?)?;
        let node = Self::node_mut(&mut self.root, &parts)?;
        debug_assert!(end >= offset);
        node.file_data.write_at(offset, data)
    }

    fn truncate(&mut self, path: &str, len: u64) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::node_mut(&mut self.root, &parts)?;
        if node.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        node.file_data.truncate(len);
        Ok(())
    }

    fn mkdir(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::Exists);
        }
        let node = Node::dir(self.alloc_inode(), mode as u16);
        self.insert_new(&parts, node)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::node_mut(&mut self.root, &parts)?;
        let typ = match node.kind {
            NodeKind::File => 0o100000,
            NodeKind::Dir => 0o040000,
            NodeKind::Symlink => 0o120000,
            NodeKind::Special => node.mode & !0o7777,
        };
        node.mode = typ | ((mode as u16) & 0o7777);
        Ok(())
    }

    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::node_mut(&mut self.root, &parts)?;
        if let Some(uid) = uid {
            node.uid = uid;
        }
        if let Some(gid) = gid {
            node.gid = gid;
        }
        Ok(())
    }

    fn setxattr(&mut self, path: &str, name: &str, value: &[u8]) -> FsResult<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let old_len = Self::node_ref(&self.root, &parts)?
            .xattrs
            .get(name)
            .map(Vec::len)
            .unwrap_or(0);
        self.ensure_capacity_delta(old_len, value.len())?;
        let node = Self::node_mut(&mut self.root, &parts)?;
        node.xattrs.insert(String::from(name), value.to_vec());
        Ok(())
    }

    fn getxattr(&self, path: &str, name: &str, buf: &mut [u8]) -> FsResult<usize> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let value = Self::node_ref(&self.root, &parts)?
            .xattrs
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

    fn listxattr(&self, path: &str, buf: &mut [u8]) -> FsResult<usize> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let mut out = Vec::new();
        for name in node.xattrs.keys() {
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }
        if buf.is_empty() {
            return Ok(out.len());
        }
        if buf.len() < out.len() {
            return Err(FsError::Io);
        }
        buf[..out.len()].copy_from_slice(&out);
        Ok(out.len())
    }

    fn removexattr(&mut self, path: &str, name: &str) -> FsResult<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let node = Self::node_mut(&mut self.root, &parts)?;
        node.xattrs.remove(name).map(|_| ()).ok_or(FsError::NotFound)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> FsResult<()> {
        let old_parts = Self::split_path(old_path)?;
        let new_parts = Self::split_path(new_path)?;
        if old_parts.is_empty() || new_parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        if new_parts.len() > old_parts.len() && new_parts.starts_with(old_parts.as_slice()) {
            return Err(FsError::InvalidPath);
        }
        let old_node = Self::node_ref(&self.root, &old_parts)?;
        if let Ok(existing) = Self::node_ref(&self.root, &new_parts) {
            if old_node.inode == existing.inode {
                return Ok(());
            }
            Self::check_rename_replacement(old_node, existing)?;
        }
        let node = {
            let (children, name) = Self::parent_mut(&mut self.root, &old_parts)?;
            children.remove(name).ok_or(FsError::NotFound)?
        };
        match Self::parent_mut(&mut self.root, &new_parts) {
            Ok((children, name)) => {
                children.remove(name);
                children.insert(String::from(name), node);
                Ok(())
            }
            Err(err) => {
                let _ = Self::parent_mut(&mut self.root, &old_parts)
                    .map(|(children, name)| children.insert(String::from(name), node));
                Err(err)
            }
        }
    }

    fn hardlink(&mut self, existing_path: &str, new_path: &str) -> FsResult<()> {
        let existing_parts = Self::split_path(existing_path)?;
        let new_parts = Self::split_path(new_path)?;
        let mut node = Self::node_ref(&self.root, &existing_parts)?.clone();
        if node.kind == NodeKind::Dir {
            return Err(FsError::Unsupported);
        }
        node.nlink = node.nlink.saturating_add(1);
        self.insert_new(&new_parts, node)
    }

    fn symlink(&mut self, link_path: &str, target: &str) -> FsResult<()> {
        let parts = Self::split_path(link_path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let node = Node::symlink(self.alloc_inode(), target);
        self.insert_new(&parts, node)
    }

    fn mknod(&mut self, path: &str, mode: u32, rdev: u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let node = Node::special(self.alloc_inode(), mode, rdev);
        self.insert_new(&parts, node)
    }

    fn exists(&self, path: &str) -> FsResult<bool> {
        let parts = Self::split_path(path)?;
        Ok(Self::node_ref(&self.root, &parts).is_ok())
    }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let parts = Self::split_path(path)?;
        Ok(Self::node_ref(&self.root, &parts)?.metadata())
    }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        if node.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        node.file_data.materialize()
    }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        if node.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        node.file_data.read_at(offset, buf)
    }

    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        if node.kind != NodeKind::Dir {
            return Err(FsError::NotAFile);
        }
        let mut out = Vec::new();
        for (name, child) in &node.children {
            let node_type = match child.kind {
                NodeKind::File => FsNodeType::File,
                NodeKind::Dir => FsNodeType::Directory,
                NodeKind::Symlink => FsNodeType::Symlink,
                NodeKind::Special => FsNodeType::Special,
            };
            out.push(FsDirEntry {
                name: name.clone(),
                node_type,
            });
        }
        Ok(out)
    }

    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        if node.kind != NodeKind::Symlink {
            return Err(FsError::NotAFile);
        }
        Ok(node.data.clone())
    }
}

/// Build a shared ramfs handle for auxiliary mounts.
pub fn new_shared_rw(limit_bytes: Option<usize>, root_mode: u16) -> SharedRwFs {
    Arc::new(Mutex::new(LocalRwFs::new(Box::new(RamFs::with_options(
        limit_bytes,
        root_mode,
    )))))
}

pub struct RamFsImpl;

pub static IMPL: RamFsImpl = RamFsImpl;

const SUPPORTED: &[FsCapability] =
    &[FsCapability::new(FsKind::RamFs, FsAccessMode::ReadWrite)];

impl FsImpl for RamFsImpl {
    fn name(&self) -> &'static str { "ramfs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    fn mount_ro(&self, _device: SharedBlockDevice) -> FsResult<SharedFs> {
        Err(FsError::Unsupported)
    }

    fn mount_rw(&self, _device: SharedBlockDevice) -> FsResult<SharedRwFs> {
        Ok(new_shared_rw(None, 0o755))
    }
}

/// Minimal runtime self-test for tree operations, content I/O, and size limits.
pub fn test() {
    let mut fs = RamFs::with_limit(RAMFS_PAGE_SIZE);
    fs.mkdir("/work", 0o755).expect("mkdir /work");
    fs.write_regular_file("/work/a", b"abc")
        .expect("write /work/a");
    let mut buf = [0u8; 2];
    let n = fs.read_range("/work/a", 1, &mut buf).expect("read range");
    assert_eq!(n, 2);
    assert_eq!(&buf, b"bc");
    assert_eq!(fs.used_bytes(), RAMFS_PAGE_SIZE);
    assert_eq!(
        fs.write_regular_file("/work/b", b"1"),
        Err(FsError::NoSpace)
    );
    fs.unlink("/work/a").expect("unlink /work/a");
    fs.rmdir("/work").expect("rmdir /work");
}

#[cfg(test)]
mod tests {
    use super::{RamFs, ReadWriteFs, SparseFile, RAMFS_PAGE_SIZE};

    #[test]
    fn large_truncate_remains_sparse() {
        let mut fs = RamFs::new();
        fs.write_regular_file("/image", &[]).unwrap();
        fs.truncate("/image", 300 * 1024 * 1024).unwrap();

        assert_eq!(fs.metadata("/image").unwrap().size, 300 * 1024 * 1024);
        assert_eq!(fs.used_bytes(), 0);
        let mut tail = [0xFF; 32];
        assert_eq!(fs.read_range("/image", 300 * 1024 * 1024 - 32, &mut tail).unwrap(), 32);
        assert_eq!(tail, [0; 32]);
    }

    #[test]
    fn cross_page_write_preserves_holes() {
        let mut file = SparseFile::default();
        let offset = RAMFS_PAGE_SIZE as u64 - 2;
        file.write_at(offset, b"abcd").unwrap();
        assert_eq!(file.pages.len(), 2);

        let mut data = [0xFF; 8];
        assert_eq!(file.read_at(offset - 2, &mut data).unwrap(), 6);
        assert_eq!(&data[..6], b"\0\0abcd");
    }

    #[test]
    fn shrink_then_grow_does_not_reveal_old_tail() {
        let mut file = SparseFile::from_bytes(b"persist-old-tail").unwrap();
        file.truncate(7);
        file.truncate(16);

        let mut data = [0xFF; 16];
        assert_eq!(file.read_at(0, &mut data).unwrap(), data.len());
        assert_eq!(&data[..7], b"persist");
        assert_eq!(&data[7..], &[0; 9]);
    }

    #[test]
    fn size_limit_charges_allocated_pages_not_holes() {
        let mut fs = RamFs::with_limit(RAMFS_PAGE_SIZE);
        fs.write_regular_file("/sparse", &[]).unwrap();
        fs.truncate("/sparse", 64 * 1024 * 1024).unwrap();
        assert_eq!(fs.used_bytes(), 0);

        fs.write_range("/sparse", 64 * 1024 * 1024 - 1, b"x")
          .unwrap();
        assert_eq!(fs.used_bytes(), RAMFS_PAGE_SIZE);
        assert_eq!(fs.write_range("/sparse", 0, b"y"), Err(super::FsError::NoSpace));
    }
}
