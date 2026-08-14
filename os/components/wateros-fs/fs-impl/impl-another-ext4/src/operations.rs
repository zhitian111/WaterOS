use super::*;

impl ReadOnlyFs for AnotherExt4Fs {
    fn mount(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        let io_error_state = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(BlockAdapter { device, io_error : io_error_state.clone() });
        let fs = Ext4::load(backend).map_err(|error| {
            log::error!(
                "[fs::another-ext4] mount failed: code={:?} detail={:?}",
                error.code(),
                error
            );
            map_error(error)
        })?;
        let state = Some(io_error_state);
        check_backend_error(&state)?;
        self.io_error_state = state;
        self.fs = Some(fs);
        self.lookup_cache.lock().clear();
        self.negative_cache.lock().take();
        self.open_nodes.clear();
        self.orphan_nodes.clear();
        self.orphan_dir = None;
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn exists(&self, path : &str) -> FsResult<bool> {
        let result = match self.lookup(path) {
            Ok(_) => Ok(true),
            Err(FsError::NotFound) => Ok(false),
            Err(error) => Err(error),
        };
        self.check_backend()?;
        result
    }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        let result = metadata(self.get()?, self.lookup(path)?);
        self.check_backend()?;
        result
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        let result = fs.read(inode, offset as usize, buf).map_err(|error| {
            log::error!("[fs::another-ext4] read failed path={} inode={} offset={} len={} code={:?}",
                        path,
                        inode,
                        offset,
                        buf.len(),
                        error.code());
            map_error(error)
        });
        self.check_backend()?;
        result
    }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> {
        let attr = ReadOnlyFs::metadata(self, path)?;
        if attr.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        let mut data = vec![0; attr.size as usize];
        let len = ReadOnlyFs::read_range(self, path, 0, &mut data)?;
        data.truncate(len);
        Ok(data)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        let attr = fs.getattr(inode)
                     .map_err(map_error)?;
        if map_type(attr.ftype) != FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        let mut entries = Vec::new();
        for entry in fs.listdir(inode)
                       .map_err(map_error)?
        {
            let name = entry.name();
            if name == "." || name == ".." || entry.unused() {
                continue;
            }
            let child = fs.getattr(entry.inode())
                          .map_err(map_error)?;
            entries.push(FsDirEntry { name,
                                      node_type : map_type(child.ftype) });
        }
        self.check_backend()?;
        Ok(entries)
    }

    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        let attr = fs.getattr(inode)
                     .map_err(map_error)?;
        if map_type(attr.ftype) != FsNodeType::Symlink {
            return Err(FsError::NotAFile);
        }
        let mut data = vec![0; attr.size as usize];
        let len = fs.readlink(inode, 0, &mut data)
                    .map_err(map_error)?;
        self.check_backend()?;
        data.truncate(len);
        Ok(data)
    }
}

impl ReadWriteFs for AnotherExt4Fs {
    fn mount_rw(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        self.mount(device)?;
        let result = self.cleanup_stale_orphans();
        self.check_backend()?;
        result
    }
    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn sync(&mut self) -> FsResult<()> {
        self.get_mut()?.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn open_node(&mut self, path : &str) -> FsResult<FsNodeId> {
        let inode = self.lookup(path)?;
        if metadata(self.get()?, inode)?.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        let count = self.open_nodes.entry(inode).or_insert(0);
        *count = count.checked_add(1).ok_or(FsError::NoSpace)?;
        self.check_backend()?;
        Ok(FsNodeId::new(inode as u64))
    }

    fn close_node(&mut self, node : FsNodeId) -> FsResult<()> {
        let inode = self.open_inode(node)?;
        let count = *self.open_nodes.get(&inode).ok_or(FsError::NotFound)?;
        if count > 1 {
            self.open_nodes.insert(inode, count - 1);
            return Ok(());
        }
        if count == 0 {
            return Err(FsError::Io);
        }
        if let Some(name) = self.orphan_nodes.get(&inode).cloned() {
            let dir = self.orphan_dir.ok_or(FsError::Io)?;
            self.get_mut()?.unlink(dir, name.as_str()).map_err(map_error)?;
            self.get_mut()?.flush_all();
            self.check_backend()?;
            self.orphan_nodes.remove(&inode);
        }
        self.open_nodes.remove(&inode);
        Ok(())
    }

    fn metadata_node(&self, node : FsNodeId) -> FsResult<FsMetadata> {
        let result = metadata(self.get()?, self.open_inode(node)?);
        self.check_backend()?;
        result
    }

    fn read_range_node(&self,
                       node : FsNodeId,
                       offset : u64,
                       buf : &mut [u8])
                       -> FsResult<usize> {
        let result = self.get()?.read(self.open_inode(node)?, offset as usize, buf).map_err(map_error);
        self.check_backend()?;
        result
    }

    fn write_range_node(&mut self,
                        node : FsNodeId,
                        offset : u64,
                        data : &[u8])
                        -> FsResult<usize> {
        let inode = self.open_inode(node)?;
        let result = write_with_ordered_size(self.get_mut()?, inode, offset, data);
        self.check_backend()?;
        result
    }

    fn truncate_node(&mut self, node : FsNodeId, len : u64) -> FsResult<()> {
        let inode = self.open_inode(node)?;
        self.get_mut()?.setattr(inode, None, None, None, Some(len), None, None, None, None)
                       .map_err(map_error)?;
        self.get_mut()?.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn exists(&self, path : &str) -> FsResult<bool> { ReadOnlyFs::exists(self, path) }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        ReadOnlyFs::metadata(self, path)
    }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> { ReadOnlyFs::read(self, path) }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        ReadOnlyFs::read_range(self, path, offset, buf)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        ReadOnlyFs::read_dir(self, path)
    }

    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        ReadOnlyFs::read_symlink(self, path)
    }

    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> FsResult<()> {
        let mut path = String::from("/");
        path.push_str(name);
        self.write_regular_file(&path, data)
    }

    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> FsResult<()> {
        let fs = self.get_mut()?;
        let (inode, created) = match lookup(fs, path) {
            Ok(inode) => (inode, false),
            Err(FsError::NotFound) => (fs.generic_create(EXT4_ROOT_INO,
                                                         path,
                                                         InodeMode::FILE | InodeMode::ALL_RW)
                                         .map_err(map_error)?, true),
            Err(error) => return Err(error),
        };
        fs.setattr(inode, None, None, None, Some(0), None, None, None, None)
          .map_err(map_error)?;
        write_with_ordered_size(fs, inode, 0, data)?;
        fs.flush_all();
        self.check_backend()?;
        if created {
            self.cache_insert(path, inode);
        }
        Ok(())
    }

    fn unlink(&mut self, path : &str) -> FsResult<()> {
        let inode = self.lookup(path)?;
        if metadata(self.get()?, inode)?.node_type == FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        self.preserve_inode_if_open(inode)?;
        self.get_mut()?.generic_remove(EXT4_ROOT_INO, path).map_err(map_error)?;
        self.get_mut()?.flush_all();
        self.check_backend()?;
        self.cache_remove_subtree(path);
        Ok(())
    }

    fn rmdir(&mut self, path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        if metadata(fs, inode)?.node_type != FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        fs.generic_remove(EXT4_ROOT_INO, path).map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_remove_subtree(path);
        Ok(())
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> FsResult<usize> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        let result = write_with_ordered_size(fs, inode, offset, data);
        self.check_backend()?;
        result
    }

    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, None, None, Some(len), None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn mkdir(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        match lookup(fs, path) {
            Ok(_) => return Err(FsError::Exists),
            Err(FsError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let (parent, name) = parent_name(path)?;
        let parent = lookup(fs, parent)?;
        let inode = fs.mkdir(parent,
                             name,
                             InodeMode::DIRECTORY | InodeMode::from_bits_retain(mode as u16))
                      .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_insert(path, inode);
        Ok(())
    }

    fn chmod(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        let file_type = fs.getattr(inode).map_err(map_error)?.ftype;
        let mode = InodeMode::from_type_and_perm(file_type,
                                                 InodeMode::from_bits_retain(mode as u16));
        fs.setattr(inode, Some(mode), None, None, None, None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, uid, gid, None, None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn mknod(&mut self, path : &str, mode : u32, rdev : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode_mode = InodeMode::from_bits_retain(mode as u16);
        match inode_mode.file_type() {
            FileType::RegularFile | FileType::Fifo | FileType::Socket => {}
            FileType::CharacterDev | FileType::BlockDev if rdev == 0 => {}
            _ => return Err(FsError::Unsupported),
        }
        let inode = fs.generic_create(EXT4_ROOT_INO, path, inode_mode)
                      .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_insert(path, inode);
        Ok(())
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        fs.generic_rename(EXT4_ROOT_INO, old_path, new_path).map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_rename_subtree(old_path, new_path);
        Ok(())
    }

    fn hardlink(&mut self, existing_path : &str, new_path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        let child = lookup(fs, existing_path)?;
        let child_meta = metadata(fs, child)?;
        if child_meta.node_type == FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        if child_meta.node_type != FsNodeType::File {
            return Err(FsError::Unsupported);
        }

        let (parent_path, name) = parent_name(new_path)?;
        let parent = lookup(fs, parent_path)?;
        if metadata(fs, parent)?.node_type != FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        if lookup(fs, new_path).is_ok() {
            return Err(FsError::Exists);
        }

        fs.link(child, parent, name).map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_insert(new_path, child);
        Ok(())
    }
}


