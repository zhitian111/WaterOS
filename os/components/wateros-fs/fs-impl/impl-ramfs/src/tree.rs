//! 基于物理页的 ramfs 实现。
//!
//! 本 crate 负责内存目录树和文件数据；tmpfs 挂载点、根目录权限及 `size=` 限制由
//! 调用方构造实例时选择。

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

use api_v0::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsImpl, FsKind, FsMetadata, FsNodeType,
    FsResult, LocalRwFs, ReadWriteFs, SharedFs, SharedRwFs,
};
use driver_block_api_v0::SharedBlockDevice;
use frame_alloctor::OwnedPhysPage;
use spin::Mutex;

const RAMFS_PAGE_SIZE : usize = 4096;

#[derive(Default)]
struct SparseFile {
    /// 逻辑文件长度，包含未分配的 sparse hole。
    len : u64,
    /// 非零页到物理页的映射；缺失页按零读取。
    pages : BTreeMap<u64, OwnedPhysPage>,
}

impl SparseFile {
    fn allocated_bytes_for_data(data : &[u8]) -> FsResult<usize> {
        let pages = data.chunks(RAMFS_PAGE_SIZE)
                        .filter(|page| {
                            page.iter()
                                .any(|byte| *byte != 0)
                        })
                        .count();
        pages.checked_mul(RAMFS_PAGE_SIZE)
             .ok_or(FsError::NoSpace)
    }

    fn from_bytes(data : &[u8]) -> FsResult<Self> {
        let mut file = Self::default();
        file.write_at(0, data)?;
        Ok(file)
    }

    fn allocated_bytes(&self) -> usize {
        self.pages
            .len()
            .saturating_mul(RAMFS_PAGE_SIZE)
    }

    fn additional_bytes_for_write(&self, offset : u64, data : &[u8]) -> FsResult<usize> {
        let mut added_pages = 0usize;
        let mut done = 0usize;
        while done < data.len() {
            let pos = offset.checked_add(done as u64)
                            .ok_or(FsError::Io)?;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(data.len() - done);
            if !self.pages
                    .contains_key(&page_idx) &&
               data[done..done + chunk].iter()
                                       .any(|byte| *byte != 0)
            {
                added_pages = added_pages.checked_add(1)
                                         .ok_or(FsError::NoSpace)?;
            }
            done += chunk;
        }
        added_pages.checked_mul(RAMFS_PAGE_SIZE)
                   .ok_or(FsError::NoSpace)
    }

    fn write_at(&mut self, offset : u64, data : &[u8]) -> FsResult<usize> {
        // 先计算并分配所有新页，确保后续写入阶段不会因半途 OOM 留下部分状态。
        if data.is_empty() {
            return Ok(0);
        }
        let end = offset.checked_add(data.len() as u64)
                        .ok_or(FsError::Io)?;
        let mut new_pages = Vec::new();
        let mut inspected = 0usize;
        while inspected < data.len() {
            let pos = offset + inspected as u64;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(data.len() - inspected);
            if !self.pages
                    .contains_key(&page_idx) &&
               data[inspected..inspected + chunk].iter()
                                                 .any(|byte| *byte != 0)
            {
                let page = OwnedPhysPage::alloc_zeroed().map_err(|_| FsError::NoSpace)?;
                new_pages.push((page_idx, page));
            }
            inspected += chunk;
        }
        for (page_idx, page) in new_pages {
            self.pages
                .insert(page_idx, page);
        }
        let mut done = 0usize;
        while done < data.len() {
            let pos = offset + done as u64;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(data.len() - done);
            let source = &data[done..done + chunk];
            if source.iter()
                     .all(|byte| *byte == 0)
            {
                let remove = if let Some(page) = self.pages
                                                     .get_mut(&page_idx)
                {
                    let bytes = page.as_bytes_mut();
                    bytes[page_off..page_off + chunk].fill(0);
                    bytes.iter()
                         .all(|byte| *byte == 0)
                } else {
                    false
                };
                if remove {
                    self.pages
                        .remove(&page_idx);
                }
            } else {
                self.pages
                    .get_mut(&page_idx)
                    .ok_or(FsError::Io)?
                    .as_bytes_mut()[page_off..page_off + chunk]
                                                               .copy_from_slice(source);
            }
            done += chunk;
        }
        self.len = self.len.max(end);
        Ok(data.len())
    }

    fn truncate(&mut self, len : u64) {
        if len < self.len {
            let first_removed =
                len.saturating_add(RAMFS_PAGE_SIZE as u64 - 1) / RAMFS_PAGE_SIZE as u64;
            let removed : Vec<u64> = self.pages
                                         .range(first_removed..)
                                         .map(|(&page_idx, _)| page_idx)
                                         .collect();
            for page_idx in removed {
                self.pages
                    .remove(&page_idx);
            }
            let tail = (len % RAMFS_PAGE_SIZE as u64) as usize;
            if tail != 0 {
                let page_idx = len / RAMFS_PAGE_SIZE as u64;
                let remove = if let Some(page) = self.pages
                                                     .get_mut(&page_idx)
                {
                    let bytes = page.as_bytes_mut();
                    bytes[tail..].fill(0);
                    bytes.iter()
                         .all(|byte| *byte == 0)
                } else {
                    false
                };
                if remove {
                    self.pages
                        .remove(&page_idx);
                }
            }
        }
        self.len = len;
    }

    fn read_at(&self, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        // EOF、空缓冲区和 sparse hole 都返回零填充的短读，不访问不存在的物理页。
        if offset >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let available =
            usize::try_from((self.len - offset).min(buf.len() as u64)).map_err(|_| FsError::Io)?;
        buf[..available].fill(0);
        let mut done = 0usize;
        while done < available {
            let pos = offset + done as u64;
            let page_idx = pos / RAMFS_PAGE_SIZE as u64;
            let page_off = (pos % RAMFS_PAGE_SIZE as u64) as usize;
            let chunk = (RAMFS_PAGE_SIZE - page_off).min(available - done);
            if let Some(page) = self.pages
                                    .get(&page_idx)
            {
                buf[done..done + chunk].copy_from_slice(&page.as_bytes()
                                                            [page_off..page_off + chunk]);
            }
            done += chunk;
        }
        Ok(available)
    }

    fn materialize(&self) -> FsResult<Vec<u8>> {
        let len = usize::try_from(self.len).map_err(|_| FsError::NoSpace)?;
        let mut data = Vec::new();
        data.try_reserve_exact(len)
            .map_err(|_| FsError::NoSpace)?;
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

struct Inode {
    kind : NodeKind,
    file_data : SparseFile,
    data : Vec<u8>,
    mode : u16,
    number : u64,
    nlink : u32,
    uid : u32,
    gid : u32,
    xattrs : BTreeMap<String, Vec<u8>>,
}

impl Inode {
    fn accounted_bytes(&self) -> usize {
        let data = match self.kind {
            NodeKind::File => self.file_data
                                  .allocated_bytes(),
            NodeKind::Symlink => self.data.len(),
            NodeKind::Dir | NodeKind::Special => 0,
        };
        data.saturating_add(self.xattrs
                                .values()
                                .map(Vec::len)
                                .sum::<usize>())
    }

    fn resident_pages(&self) -> usize {
        if self.kind == NodeKind::File {
            self.file_data
                .pages
                .len()
        } else {
            0
        }
    }
}

#[derive(Clone)]
struct Node {
    inode : Arc<Mutex<Inode>>,
    children : BTreeMap<String, Node>,
}

impl Node {
    fn dir(inode : u64, mode : u16) -> Self {
        Self { inode : Arc::new(Mutex::new(Inode { kind : NodeKind::Dir,
                                                   file_data : SparseFile::default(),
                                                   data : Vec::new(),
                                                   mode : 0o040000 | (mode & 0o7777),
                                                   number : inode,
                                                   nlink : 2,
                                                   uid : 0,
                                                   gid : 0,
                                                   xattrs : BTreeMap::new() })),
               children : BTreeMap::new() }
    }

    fn file(inode : u64, data : &[u8]) -> FsResult<Self> {
        Ok(Self { inode : Arc::new(Mutex::new(Inode { kind : NodeKind::File,
                                                      file_data:
                                                          SparseFile::from_bytes(data)?,
                                                      data : Vec::new(),
                                                      mode : 0o100644,
                                                      number : inode,
                                                      nlink : 1,
                                                      uid : 0,
                                                      gid : 0,
                                                      xattrs : BTreeMap::new() })),
                  children : BTreeMap::new() })
    }

    fn symlink(inode : u64, target : &str) -> Self {
        Self { inode : Arc::new(Mutex::new(Inode { kind : NodeKind::Symlink,
                                                   file_data : SparseFile::default(),
                                                   data : target.as_bytes()
                                                                .to_vec(),
                                                   mode : 0o120777,
                                                   number : inode,
                                                   nlink : 1,
                                                   uid : 0,
                                                   gid : 0,
                                                   xattrs : BTreeMap::new() })),
               children : BTreeMap::new() }
    }

    fn special(inode : u64, mode : u32, _rdev : u32) -> Self {
        Self { inode : Arc::new(Mutex::new(Inode { kind : NodeKind::Special,
                                                   file_data : SparseFile::default(),
                                                   data : Vec::new(),
                                                   mode : mode as u16,
                                                   number : inode,
                                                   nlink : 1,
                                                   uid : 0,
                                                   gid : 0,
                                                   xattrs : BTreeMap::new() })),
               children : BTreeMap::new() }
    }

    fn kind(&self) -> NodeKind {
        self.inode
            .lock()
            .kind
    }

    fn inode_number(&self) -> u64 {
        self.inode
            .lock()
            .number
    }

    fn own_accounted_bytes(&self) -> usize {
        self.inode
            .lock()
            .accounted_bytes()
    }

    fn accounted_bytes_seen(&self, seen : &mut BTreeSet<u64>) -> usize {
        let number = self.inode_number();
        let own = if seen.insert(number) {
            self.own_accounted_bytes()
        } else {
            0
        };
        own +
        self.children
            .values()
            .map(|child| child.accounted_bytes_seen(seen))
            .sum::<usize>()
    }

    fn resident_pages_seen(&self, seen : &mut BTreeSet<u64>) -> usize {
        let number = self.inode_number();
        let own = if seen.insert(number) {
            self.inode
                .lock()
                .resident_pages()
        } else {
            0
        };
        own +
        self.children
            .values()
            .map(|child| child.resident_pages_seen(seen))
            .sum::<usize>()
    }

    fn metadata(&self) -> FsMetadata {
        let inode = self.inode.lock();
        let node_type = match inode.kind {
            NodeKind::File => FsNodeType::File,
            NodeKind::Dir => FsNodeType::Directory,
            NodeKind::Symlink => FsNodeType::Symlink,
            NodeKind::Special => FsNodeType::Special,
        };
        FsMetadata { node_type,
                     size : match inode.kind {
                         NodeKind::File => inode.file_data.len,
                         NodeKind::Symlink => inode.data.len() as u64,
                         NodeKind::Dir | NodeKind::Special => 0,
                     },
                     mode : inode.mode,
                     inode : inode.number,
                     nlink : inode.nlink,
                     uid : inode.uid,
                     gid : inode.gid }
    }
}

struct OpenNode {
    inode : Arc<Mutex<Inode>>,
    refs : usize,
}

/// Physical-page-backed ramfs tree.
pub struct RamFs {
    root : Node,
    open_nodes : BTreeMap<u64, OpenNode>,
    next_inode : u64,
    mounted : bool,
    limit_bytes : Option<usize>,
}

impl RamFs {
    /// 创建不限制容量、由物理页支持且根目录权限为 `0755` 的 ramfs。
    pub fn new() -> Self { Self::with_options(None, 0o755) }

    /// 创建由物理页支持并按数据字节计费、带容量上限的 ramfs。
    pub fn with_limit(limit_bytes : usize) -> Self { Self::with_options(Some(limit_bytes), 0o755) }

    /// 创建显式指定容量上限和根目录模式、由物理页支持的 ramfs。
    pub fn with_options(limit_bytes : Option<usize>, root_mode : u16) -> Self {
        Self { root : Node::dir(1, root_mode),
               open_nodes : BTreeMap::new(),
               next_inode : 2,
               mounted : true,
               limit_bytes }
    }

    /// 当前计入文件内容、符号链接目标和扩展属性值的字节数。
    pub fn used_bytes(&self) -> usize {
        let mut seen = BTreeSet::new();
        let mut used = self.root
                           .accounted_bytes_seen(&mut seen);
        for (&number, open) in &self.open_nodes {
            if seen.insert(number) {
                let inode = open.inode.lock();
                used = used.saturating_add(inode.accounted_bytes());
            }
        }
        used
    }

    /// 由仍被链接或仍打开的文件拥有的物理载荷页数量。
    pub fn resident_pages(&self) -> usize {
        let mut seen = BTreeSet::new();
        let mut pages = self.root
                            .resident_pages_seen(&mut seen);
        for (&number, open) in &self.open_nodes {
            if seen.insert(number) {
                pages = pages.saturating_add(open.inode
                                                 .lock()
                                                 .resident_pages());
            }
        }
        pages
    }

    /// 配置的最大计费字节数；未限制时为 `None`。
    pub fn limit_bytes(&self) -> Option<usize> { self.limit_bytes }

    fn alloc_inode(&mut self) -> u64 {
        let inode = self.next_inode;
        self.next_inode += 1;
        inode
    }

    fn split_path(path : &str) -> FsResult<Vec<&str>> {
        let p = path.trim();
        let p = p.strip_prefix('/')
                 .unwrap_or(p);
        if p.is_empty() {
            return Ok(Vec::new());
        }
        Ok(p.split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect())
    }

    fn node_ref<'a>(root : &'a Node, parts : &[&str]) -> FsResult<&'a Node> {
        let mut node = root;
        for part in parts {
            if node.kind() != NodeKind::Dir {
                return Err(FsError::NotFound);
            }
            node = node.children
                       .get(*part)
                       .ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

    fn node_mut<'a>(root : &'a mut Node, parts : &[&str]) -> FsResult<&'a mut Node> {
        let mut node = root;
        for part in parts {
            if node.kind() != NodeKind::Dir {
                return Err(FsError::NotFound);
            }
            node = node.children
                       .get_mut(*part)
                       .ok_or(FsError::NotFound)?;
        }
        Ok(node)
    }

    fn parent_mut<'a>(root : &'a mut Node,
                      parts : &[&'a str])
                      -> FsResult<(&'a mut BTreeMap<String, Node>, &'a str)> {
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let name = parts[parts.len() - 1];
        let parent = Self::node_mut(root, &parts[..parts.len() - 1])?;
        if parent.kind() != NodeKind::Dir {
            return Err(FsError::NotFound);
        }
        Ok((&mut parent.children, name))
    }

    fn ensure_capacity_additional(&self, additional : usize) -> FsResult<()> {
        let next = self.used_bytes()
                       .checked_add(additional)
                       .ok_or(FsError::NoSpace)?;
        match self.limit_bytes {
            Some(limit) if next > limit => Err(FsError::NoSpace),
            _ => Ok(()),
        }
    }

    fn ensure_capacity_replace(&self, old : Option<&Node>, new_bytes : usize) -> FsResult<()> {
        let released = old.map(|node| {
                              let inode = node.inode.lock();
                              if inode.nlink == 1 {
                                  inode.accounted_bytes()
                              } else {
                                  0
                              }
                          })
                          .unwrap_or(0);
        let next = self.used_bytes()
                       .saturating_sub(released)
                       .checked_add(new_bytes)
                       .ok_or(FsError::NoSpace)?;
        match self.limit_bytes {
            Some(limit) if next > limit => Err(FsError::NoSpace),
            _ => Ok(()),
        }
    }

    fn ensure_capacity_delta(&self, old_bytes : usize, new_bytes : usize) -> FsResult<()> {
        if new_bytes <= old_bytes {
            return Ok(());
        }
        let next = self.used_bytes()
                       .checked_add(new_bytes - old_bytes)
                       .ok_or(FsError::NoSpace)?;
        match self.limit_bytes {
            Some(limit) if next > limit => Err(FsError::NoSpace),
            _ => Ok(()),
        }
    }

    fn insert_new(&mut self, parts : &[&str], node : Node) -> FsResult<()> {
        self.ensure_capacity_additional(node.own_accounted_bytes())?;
        let (children, name) = Self::parent_mut(&mut self.root, parts)?;
        if children.contains_key(name) {
            return Err(FsError::Exists);
        }
        children.insert(String::from(name), node);
        Ok(())
    }

    fn drop_link(node : &Node) {
        let mut inode = node.inode.lock();
        inode.nlink = inode.nlink
                           .saturating_sub(1);
    }

    fn check_rename_replacement(old_node : &Node, new_node : &Node) -> FsResult<()> {
        match (old_node.kind(), new_node.kind()) {
            (NodeKind::Dir, NodeKind::Dir)
                if new_node.children
                           .is_empty() =>
            {
                Ok(())
            }
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


#[path = "tree/operations.rs"]
mod operations;
pub use operations::{new_shared_rw, test, IMPL, RamFsImpl};
#[cfg(feature = "self_test")]
pub use operations::self_test;
