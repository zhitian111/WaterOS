use super::*;

impl ReadWriteFs for RamFs {
    fn mount_rw(&mut self, _device : SharedBlockDevice) -> FsResult<()> {
        self.mounted = true;
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.mounted }

    fn sync(&mut self) -> FsResult<()> {
        // Data in ramfs is already committed to its in-memory tree.
        Ok(())
    }

    fn open_node(&mut self, path : &str) -> FsResult<api_v0::FsNodeId> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let number = node.inode_number();
        let inode = node.inode.clone();
        self.open_nodes
            .entry(number)
            .and_modify(|open| {
                open.refs = open.refs
                                .saturating_add(1)
            })
            .or_insert(OpenNode { inode, refs : 1 });
        Ok(api_v0::FsNodeId::new(number))
    }

    fn close_node(&mut self, node : api_v0::FsNodeId) -> FsResult<()> {
        let number = node.raw();
        let remove = {
            let open = self.open_nodes
                           .get_mut(&number)
                           .ok_or(FsError::NotFound)?;
            open.refs = open.refs
                            .checked_sub(1)
                            .ok_or(FsError::Io)?;
            open.refs == 0
        };
        if remove {
            self.open_nodes
                .remove(&number);
        }
        Ok(())
    }

    fn metadata_node(&self, node : api_v0::FsNodeId) -> FsResult<FsMetadata> {
        let inode = self.open_nodes
                        .get(&node.raw())
                        .ok_or(FsError::NotFound)?
                        .inode
                        .lock();
        let node_type = match inode.kind {
            NodeKind::File => FsNodeType::File,
            NodeKind::Dir => FsNodeType::Directory,
            NodeKind::Symlink => FsNodeType::Symlink,
            NodeKind::Special => FsNodeType::Special,
        };
        Ok(FsMetadata { node_type,
                        size : match inode.kind {
                            NodeKind::File => inode.file_data.len,
                            NodeKind::Symlink => inode.data.len() as u64,
                            NodeKind::Dir | NodeKind::Special => 0,
                        },
                        mode : inode.mode,
                        inode : inode.number,
                        nlink : inode.nlink,
                        uid : inode.uid,
                        gid : inode.gid })
    }

    fn read_range_node(&self,
                       node : api_v0::FsNodeId,
                       offset : u64,
                       buf : &mut [u8])
                       -> FsResult<usize> {
        let inode = self.open_nodes
                        .get(&node.raw())
                        .ok_or(FsError::NotFound)?
                        .inode
                        .lock();
        if inode.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        inode.file_data
             .read_at(offset, buf)
    }

    fn write_range_node(&mut self,
                        node : api_v0::FsNodeId,
                        offset : u64,
                        data : &[u8])
                        -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let inode_arc = self.open_nodes
                            .get(&node.raw())
                            .ok_or(FsError::NotFound)?
                            .inode
                            .clone();
        let (old_bytes, added) = {
            let inode = inode_arc.lock();
            if inode.kind != NodeKind::File {
                return Err(FsError::NotAFile);
            }
            (inode.file_data
                  .allocated_bytes(),
             inode.file_data
                  .additional_bytes_for_write(offset, data)?)
        };
        self.ensure_capacity_delta(old_bytes,
                                   old_bytes.checked_add(added)
                                            .ok_or(FsError::NoSpace)?)?;
        inode_arc.lock()
                 .file_data
                 .write_at(offset, data)
    }

    fn truncate_node(&mut self, node : api_v0::FsNodeId, len : u64) -> FsResult<()> {
        let mut inode = self.open_nodes
                            .get(&node.raw())
                            .ok_or(FsError::NotFound)?
                            .inode
                            .lock();
        if inode.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        inode.file_data
             .truncate(len);
        Ok(())
    }

    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> FsResult<()> {
        self.write_regular_file(alloc::format!("/{name}").as_str(), data)
    }

    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        if matches!(Self::node_ref(&self.root, &parts).ok()
                                                      .map(Node::kind),
                    Some(NodeKind::Dir))
        {
            return Err(FsError::NotAFile);
        }
        let new_bytes = SparseFile::allocated_bytes_for_data(data)?;
        self.ensure_capacity_replace(Self::node_ref(&self.root, &parts).ok(),
                                     new_bytes)?;
        let node = Node::file(self.alloc_inode(), data)?;
        let (children, name) = Self::parent_mut(&mut self.root, &parts)?;
        if let Some(replaced) = children.insert(String::from(name), node) {
            Self::drop_link(&replaced);
        }
        Ok(())
    }

    fn unlink(&mut self, path : &str) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let (children, name) = Self::parent_mut(&mut self.root, &parts)?;
        let node = children.get(name)
                           .ok_or(FsError::NotFound)?;
        if node.kind() == NodeKind::Dir {
            return Err(FsError::NotAFile);
        }
        let removed = children.remove(name)
                              .ok_or(FsError::NotFound)?;
        Self::drop_link(&removed);
        Ok(())
    }

    fn rmdir(&mut self, path : &str) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let (children, name) = Self::parent_mut(&mut self.root, &parts)?;
        let node = children.get(name)
                           .ok_or(FsError::NotFound)?;
        if node.kind() != NodeKind::Dir {
            return Err(FsError::NotAFile);
        }
        if !node.children
                .is_empty()
        {
            return Err(FsError::NotEmpty);
        }
        children.remove(name);
        Ok(())
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let parts = Self::split_path(path)?;
        let inode_arc = {
            let node = Self::node_ref(&self.root, &parts)?;
            if node.kind() != NodeKind::File {
                return Err(FsError::NotAFile);
            }
            node.inode.clone()
        };
        let (old_bytes, added) = {
            let inode = inode_arc.lock();
            (inode.file_data
                  .allocated_bytes(),
             inode.file_data
                  .additional_bytes_for_write(offset, data)?)
        };
        self.ensure_capacity_delta(old_bytes,
                                   old_bytes.checked_add(added)
                                            .ok_or(FsError::NoSpace)?)?;
        inode_arc.lock()
                 .file_data
                 .write_at(offset, data)
    }

    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let mut inode = node.inode.lock();
        if inode.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        inode.file_data
             .truncate(len);
        Ok(())
    }

    fn mkdir(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::Exists);
        }
        let node = Node::dir(self.alloc_inode(), mode as u16);
        self.insert_new(&parts, node)
    }

    fn chmod(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let mut inode = node.inode.lock();
        let typ = match inode.kind {
            NodeKind::File => 0o100000,
            NodeKind::Dir => 0o040000,
            NodeKind::Symlink => 0o120000,
            NodeKind::Special => inode.mode & !0o7777,
        };
        inode.mode = typ | ((mode as u16) & 0o7777);
        Ok(())
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let mut inode = node.inode.lock();
        if let Some(uid) = uid {
            inode.uid = uid;
        }
        if let Some(gid) = gid {
            inode.gid = gid;
        }
        Ok(())
    }

    fn setxattr(&mut self, path : &str, name : &str, value : &[u8]) -> FsResult<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let inode_arc = Self::node_ref(&self.root, &parts)?.inode
                                                           .clone();
        let old_len = inode_arc.lock()
                               .xattrs
                               .get(name)
                               .map(Vec::len)
                               .unwrap_or(0);
        self.ensure_capacity_delta(old_len, value.len())?;
        inode_arc.lock()
                 .xattrs
                 .insert(String::from(name), value.to_vec());
        Ok(())
    }

    fn getxattr(&self, path : &str, name : &str, buf : &mut [u8]) -> FsResult<usize> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let inode = node.inode.lock();
        let value = inode.xattrs
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

    fn listxattr(&self, path : &str, buf : &mut [u8]) -> FsResult<usize> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let inode = node.inode.lock();
        let mut out = Vec::new();
        for name in inode.xattrs.keys() {
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

    fn removexattr(&mut self, path : &str, name : &str) -> FsResult<()> {
        if name.is_empty() || name.contains('\0') {
            return Err(FsError::InvalidPath);
        }
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let result = node.inode
                         .lock()
                         .xattrs
                         .remove(name)
                         .map(|_| ())
                         .ok_or(FsError::NotFound);
        result
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> FsResult<()> {
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
            if old_node.inode_number() == existing.inode_number() {
                return Ok(());
            }
            Self::check_rename_replacement(old_node, existing)?;
        }
        let node = {
            let (children, name) = Self::parent_mut(&mut self.root, &old_parts)?;
            children.remove(name)
                    .ok_or(FsError::NotFound)?
        };
        match Self::parent_mut(&mut self.root, &new_parts) {
            Ok((children, name)) => {
                if let Some(replaced) = children.remove(name) {
                    Self::drop_link(&replaced);
                }
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

    fn hardlink(&mut self, existing_path : &str, new_path : &str) -> FsResult<()> {
        let existing_parts = Self::split_path(existing_path)?;
        let new_parts = Self::split_path(new_path)?;
        if new_parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let node = Self::node_ref(&self.root, &existing_parts)?.clone();
        if node.kind() == NodeKind::Dir {
            return Err(FsError::Unsupported);
        }
        {
            let (children, name) = Self::parent_mut(&mut self.root, &new_parts)?;
            if children.contains_key(name) {
                return Err(FsError::Exists);
            }
        }
        {
            let mut inode = node.inode.lock();
            inode.nlink = inode.nlink
                               .checked_add(1)
                               .ok_or(FsError::NoSpace)?;
        }
        let (children, name) = Self::parent_mut(&mut self.root, &new_parts)?;
        children.insert(String::from(name), node);
        Ok(())
    }

    fn symlink(&mut self, link_path : &str, target : &str) -> FsResult<()> {
        let parts = Self::split_path(link_path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let node = Node::symlink(self.alloc_inode(), target);
        self.insert_new(&parts, node)
    }

    fn mknod(&mut self, path : &str, mode : u32, rdev : u32) -> FsResult<()> {
        let parts = Self::split_path(path)?;
        if parts.is_empty() {
            return Err(FsError::InvalidPath);
        }
        let node = Node::special(self.alloc_inode(), mode, rdev);
        self.insert_new(&parts, node)
    }

    fn exists(&self, path : &str) -> FsResult<bool> {
        let parts = Self::split_path(path)?;
        Ok(Self::node_ref(&self.root, &parts).is_ok())
    }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        let parts = Self::split_path(path)?;
        Ok(Self::node_ref(&self.root, &parts)?.metadata())
    }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let inode = node.inode.lock();
        if inode.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        inode.file_data
             .materialize()
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let inode = node.inode.lock();
        if inode.kind != NodeKind::File {
            return Err(FsError::NotAFile);
        }
        inode.file_data
             .read_at(offset, buf)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        if node.kind() != NodeKind::Dir {
            return Err(FsError::NotAFile);
        }
        let mut out = Vec::new();
        for (name, child) in &node.children {
            let node_type = match child.kind() {
                NodeKind::File => FsNodeType::File,
                NodeKind::Dir => FsNodeType::Directory,
                NodeKind::Symlink => FsNodeType::Symlink,
                NodeKind::Special => FsNodeType::Special,
            };
            out.push(FsDirEntry { name : name.clone(),
                                  node_type });
        }
        Ok(out)
    }

    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        let parts = Self::split_path(path)?;
        let node = Self::node_ref(&self.root, &parts)?;
        let inode = node.inode.lock();
        if inode.kind != NodeKind::Symlink {
            return Err(FsError::NotAFile);
        }
        Ok(inode.data.clone())
    }
}

/// Build a shared ramfs handle for auxiliary mounts.
pub fn new_shared_rw(limit_bytes : Option<usize>, root_mode : u16) -> SharedRwFs {
    Arc::new(Mutex::new(LocalRwFs::new(Box::new(RamFs::with_options(limit_bytes, root_mode)))))
}

pub struct RamFsImpl;

pub static IMPL : RamFsImpl = RamFsImpl;

const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::RamFs, FsAccessMode::ReadWrite)];

impl FsImpl for RamFsImpl {
    fn name(&self) -> &'static str { "ramfs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    fn mount_ro(&self, _device : SharedBlockDevice) -> FsResult<SharedFs> {
        Err(FsError::Unsupported)
    }

    fn mount_rw(&self, _device : SharedBlockDevice) -> FsResult<SharedRwFs> {
        Ok(new_shared_rw(None, 0o755))
    }
}

/// Runtime self-test for sparse I/O, shared inode lifetime, size limits, and page reclaim.
pub fn test() {
    let frames_before = frame_alloctor::frame_mem_stats();
    let mut fs = RamFs::with_limit(RAMFS_PAGE_SIZE);
    fs.mkdir("/work", 0o755)
      .expect("mkdir /work");
    fs.write_regular_file("/work/a", b"abc")
      .expect("write /work/a");
    let open = fs.open_node("/work/a")
                 .expect("open /work/a");
    fs.hardlink("/work/a", "/work/b")
      .expect("hardlink /work/b");
    assert_eq!(fs.metadata("/work/a")
                 .expect("metadata /work/a")
                 .nlink,
               2);
    fs.write_range("/work/b", 1, b"Z")
      .expect("write through hardlink");
    let mut buf = [0u8; 2];
    let n = fs.read_range_node(open, 1, &mut buf)
              .expect("read stable node");
    assert_eq!(n, 2);
    assert_eq!(&buf, b"Zc");
    assert_eq!(fs.used_bytes(), RAMFS_PAGE_SIZE);
    assert_eq!(fs.resident_pages(), 1);
    assert_eq!(fs.write_range("/work/a", RAMFS_PAGE_SIZE as u64, b"1"),
               Err(FsError::NoSpace));
    assert_eq!(fs.read("/work/a")
                 .expect("read preserved content"),
               b"aZc");
    fs.unlink("/work/a")
      .expect("unlink /work/a");
    fs.unlink("/work/b")
      .expect("unlink /work/b");
    assert_eq!(fs.read_range_node(open, 0, &mut buf)
                 .expect("read unlinked open node"),
               2);
    assert_eq!(fs.resident_pages(), 1);
    fs.close_node(open)
      .expect("close stable node");
    assert_eq!(fs.resident_pages(), 0);

    fs.write_regular_file("/work/sparse", &[])
      .expect("create sparse file");
    fs.truncate("/work/sparse", 300 * 1024 * 1024)
      .expect("sparse truncate");
    assert_eq!(fs.resident_pages(), 0);
    let mut hole = [0xFF; 16];
    assert_eq!(fs.read_range("/work/sparse",
                             300 * 1024 * 1024 - 16,
                             &mut hole)
                 .expect("read sparse tail"),
               hole.len());
    assert_eq!(hole, [0; 16]);
    fs.unlink("/work/sparse")
      .expect("unlink sparse file");
    fs.rmdir("/work")
      .expect("rmdir /work");
    let frames_after = frame_alloctor::frame_mem_stats();
    log::info!("[ramfs-test] frames_before={} frames_after={} resident_pages={}",
               frames_before.free_frames,
               frames_after.free_frames,
               fs.resident_pages());
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    test();
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
        fs.write_range("/sparse", 64 * 1024 * 1024 - 1, b"x").unwrap();
        assert_eq!(fs.used_bytes(), RAMFS_PAGE_SIZE);
        assert_eq!(fs.write_range("/sparse", 0, b"y"), Err(FsError::NoSpace));
    }
}
