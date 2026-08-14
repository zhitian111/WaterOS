use super::*;

pub struct AnotherExt4Fs {
    pub(crate) fs : Option<Ext4>,
    pub(crate) io_error_state : Option<BackendErrorState>,
    pub(crate) lookup_cache : Mutex<BTreeMap<String, u32>>,
    pub(crate) negative_cache : Mutex<Option<Box<NegativeDentryCache>>>,
    pub(crate) open_nodes : BTreeMap<u32, usize>,
    pub(crate) orphan_nodes : BTreeMap<u32, String>,
    pub(crate) orphan_dir : Option<u32>,
}

impl AnotherExt4Fs {
    pub(crate) const fn new() -> Self {
        Self { fs : None,
               io_error_state : None,
               lookup_cache : Mutex::new(BTreeMap::new()),
               negative_cache : Mutex::new(None),
               open_nodes : BTreeMap::new(),
               orphan_nodes : BTreeMap::new(),
               orphan_dir : None }
    }
    pub(crate) fn get(&self) -> FsResult<&Ext4> {
        self.check_backend()?;
        self.fs
            .as_ref()
            .ok_or(FsError::NotMounted)
    }

    pub(crate) fn get_mut(&mut self) -> FsResult<&mut Ext4> {
        self.check_backend()?;
        self.fs.as_mut().ok_or(FsError::NotMounted)
    }

    pub(crate) fn check_backend(&self) -> FsResult<()> {
        check_backend_error(&self.io_error_state)
    }

    pub(crate) fn lookup(&self, path : &str) -> FsResult<u32> {
        if let Some(inode) = self.lookup_cache.lock().get(path).copied() {
            lookup_diag_event!(positive_hit);
            return Ok(inode);
        }
        if self.negative_cache
               .lock()
               .as_ref()
               .is_some_and(|cache| cache.contains(path))
        {
            lookup_diag_event!(negative_hit);
            return Err(FsError::NotFound);
        }
        match lookup(self.get()?, path) {
            Ok(inode) => {
                lookup_diag_event!(lookup_success);
                self.cache_insert(path, inode);
                Ok(inode)
            }
            Err(FsError::NotFound) => {
                lookup_diag_event!(not_found);
                self.negative_cache
                    .lock()
                    .get_or_insert_with(|| Box::new(NegativeDentryCache::new()))
                    .insert(path);
                Err(FsError::NotFound)
            }
            Err(error) => Err(error),
        }
    }

    pub(crate) fn cache_insert(&self, path : &str, inode : u32) {
        let mut cache = self.lookup_cache.lock();
        if cache.len() >= LOOKUP_CACHE_CAPACITY && !cache.contains_key(path) {
            cache.clear();
            lookup_diag_positive_clear();
        }
        cache.insert(String::from(path), inode);
        drop(cache);
        self.negative_cache_remove_exact(path);
    }

    pub(crate) fn negative_cache_remove_exact(&self, path : &str) {
        let removed = self.negative_cache
                          .lock()
                          .as_mut()
                          .map(|cache| cache.remove_exact(path))
                          .unwrap_or(0);
        lookup_diag_negative_invalidate(removed);
    }

    pub(crate) fn negative_cache_remove_subtree(&self, path : &str) {
        let removed = self.negative_cache
                          .lock()
                          .as_mut()
                          .map(|cache| cache.remove_subtree(path))
                          .unwrap_or(0);
        lookup_diag_negative_invalidate(removed);
    }

    pub(crate) fn cache_remove_subtree(&self, path : &str) {
        let prefix = if path.ends_with('/') {
            String::from(path)
        } else {
            let mut prefix = String::from(path);
            prefix.push('/');
            prefix
        };
        self.lookup_cache
            .lock()
            .retain(|cached, _| cached != path && !cached.starts_with(prefix.as_str()));
    }

    pub(crate) fn cache_rename_subtree(&self, old_path : &str, new_path : &str) {
        let old_prefix = if old_path.ends_with('/') {
            String::from(old_path)
        } else {
            let mut prefix = String::from(old_path);
            prefix.push('/');
            prefix
        };
        let new_prefix = if new_path.ends_with('/') {
            String::from(new_path)
        } else {
            let mut prefix = String::from(new_path);
            prefix.push('/');
            prefix
        };
        let mut moved = Vec::new();
        let mut cache = self.lookup_cache.lock();
        cache.retain(|cached, inode| {
            if cached == old_path {
                moved.push((String::from(new_path), *inode));
                return false;
            }
            if let Some(suffix) = cached.strip_prefix(old_prefix.as_str()) {
                let mut renamed = String::from(new_path.trim_end_matches('/'));
                renamed.push('/');
                renamed.push_str(suffix);
                moved.push((renamed, *inode));
                return false;
            }
            cached != new_path && !cached.starts_with(new_prefix.as_str())
        });
        for (path, inode) in moved {
            cache.insert(path, inode);
        }
        drop(cache);
        self.negative_cache_remove_subtree(new_path);
    }

    pub(crate) fn open_inode(&self, node : FsNodeId) -> FsResult<u32> {
        let inode = u32::try_from(node.raw()).map_err(|_| FsError::InvalidPath)?;
        self.open_nodes
            .contains_key(&inode)
            .then_some(inode)
            .ok_or(FsError::NotFound)
    }

    pub(crate) fn cleanup_stale_orphans(&mut self) -> FsResult<()> {
        let fs = self.get_mut()?;
        let dir = match lookup(fs, OPEN_INODE_DIR) {
            Ok(dir) => dir,
            Err(FsError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        if metadata(fs, dir)?.node_type != FsNodeType::Directory {
            return Err(FsError::InvalidPath);
        }
        let names : Vec<String> = fs.listdir(dir)
                                    .map_err(map_error)?
                                    .into_iter()
                                    .filter(|entry| {
                                        let name = entry.name();
                                        name != "." && name != ".." && !entry.unused()
                                    })
                                    .map(|entry| entry.name())
                                    .collect();
        let had_stale = !names.is_empty();
        for name in names {
            fs.unlink(dir, name.as_str()).map_err(map_error)?;
        }
        if had_stale {
            fs.flush_all();
        }
        self.orphan_dir = Some(dir);
        Ok(())
    }

    pub(crate) fn ensure_orphan_dir(&mut self) -> FsResult<u32> {
        if let Some(dir) = self.orphan_dir {
            return Ok(dir);
        }
        let fs = self.get_mut()?;
        let (dir, created) = match lookup(fs, OPEN_INODE_DIR) {
            Ok(dir) => (dir, false),
            Err(FsError::NotFound) => {
                (fs.mkdir(EXT4_ROOT_INO,
                          OPEN_INODE_DIR.trim_start_matches('/'),
                          InodeMode::DIRECTORY | InodeMode::from_bits_retain(0o700))
                   .map_err(map_error)?,
                 true)
            }
            Err(error) => return Err(error),
        };
        if metadata(fs, dir)?.node_type != FsNodeType::Directory {
            return Err(FsError::InvalidPath);
        }
        self.orphan_dir = Some(dir);
        if created {
            self.cache_insert(OPEN_INODE_DIR, dir);
        }
        Ok(dir)
    }

    pub(crate) fn preserve_inode_if_open(&mut self, inode : u32) -> FsResult<()> {
        if !self.open_nodes.contains_key(&inode) || self.orphan_nodes.contains_key(&inode) {
            return Ok(());
        }
        let dir = self.ensure_orphan_dir()?;
        let name = alloc::format!("{inode:08x}");
        self.get_mut()?.link(inode, dir, name.as_str()).map_err(map_error)?;
        self.get_mut()?.flush_all();
        let mut path = String::from(OPEN_INODE_DIR);
        path.push('/');
        path.push_str(name.as_str());
        self.orphan_nodes.insert(inode, name);
        self.cache_insert(path.as_str(), inode);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{AnotherExt4Fs, FsNodeId, ReadWriteFs};
    use alloc::boxed::Box;

    #[test]
    pub(crate) fn lookup_cache_rename_moves_only_source_subtree() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/src", 10);
        fs.cache_insert("/src/child", 11);
        fs.cache_insert("/dst/stale", 12);
        fs.cache_insert("/unrelated", 13);
        {
            let mut negative = fs.negative_cache.lock();
            let negative = negative.get_or_insert_with(|| Box::new(super::NegativeDentryCache::new()));
            negative.insert("/dst");
            negative.insert("/dst/missing");
            negative.insert("/dstish/missing");
        }

        fs.cache_rename_subtree("/src", "/dst");

        let cache = fs.lookup_cache.lock();
        assert_eq!(cache.get("/dst"), Some(&10));
        assert_eq!(cache.get("/dst/child"), Some(&11));
        assert_eq!(cache.get("/unrelated"), Some(&13));
        assert!(!cache.contains_key("/src"));
        assert!(!cache.contains_key("/src/child"));
        assert!(!cache.contains_key("/dst/stale"));
        drop(cache);
        let negative = fs.negative_cache.lock();
        let negative = negative.as_ref().unwrap();
        assert!(!negative.contains("/dst"));
        assert!(!negative.contains("/dst/missing"));
        assert!(negative.contains("/dstish/missing"));
    }

    #[test]
    pub(crate) fn stable_node_refcount_closes_exactly_once() {
        let mut fs = AnotherExt4Fs::new();
        fs.open_nodes.insert(42, 2);
        let node = FsNodeId::new(42);

        fs.close_node(node).unwrap();
        assert_eq!(fs.open_nodes.get(&42), Some(&1));
        fs.close_node(node).unwrap();
        assert!(!fs.open_nodes.contains_key(&42));
        assert_eq!(fs.close_node(node), Err(FsError::NotFound));
    }

    #[test]
    pub(crate) fn lookup_cache_remove_invalidates_descendants_only() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/tmp/work", 20);
        fs.cache_insert("/tmp/work/output", 21);
        fs.cache_insert("/tmp/worker", 22);

        fs.cache_remove_subtree("/tmp/work");

        let cache = fs.lookup_cache.lock();
        assert!(!cache.contains_key("/tmp/work"));
        assert!(!cache.contains_key("/tmp/work/output"));
        assert_eq!(cache.get("/tmp/worker"), Some(&22));
    }

    #[test]
    pub(crate) fn negative_cache_requires_full_path_match_and_removes_exact_entry() {
        let mut cache = super::NegativeDentryCache::new();
        let original = "/missing/a";
        let bucket = super::NegativeDentryCache::bucket(super::negative_path_hash(original));
        let collision = (0..10_000)
                            .map(|index| alloc::format!("/collision/{index}"))
                            .find(|path| {
                                path != original &&
                                super::NegativeDentryCache::bucket(
                                    super::negative_path_hash(path.as_str()),
                                ) == bucket
                            })
                            .expect("find another path in the same cache bucket");
        cache.insert(original);
        assert!(cache.contains(original));
        assert!(!cache.contains(collision.as_str()));
        assert_eq!(cache.remove_exact(collision.as_str()), 0);
        assert_eq!(cache.remove_exact(original), 1);
        assert!(!cache.contains(original));
    }

    #[test]
    pub(crate) fn negative_cache_subtree_invalidation_preserves_prefix_sibling() {
        let mut cache = super::NegativeDentryCache::new();
        cache.insert("/tmp/work");
        cache.insert("/tmp/work/output");
        cache.insert("/tmp/worker");

        assert_eq!(cache.remove_subtree("/tmp/work"), 2);
        assert!(!cache.contains("/tmp/work"));
        assert!(!cache.contains("/tmp/work/output"));
        assert!(cache.contains("/tmp/worker"));
    }

    #[test]
    pub(crate) fn positive_cache_publication_invalidates_matching_negative_entry() {
        let fs = AnotherExt4Fs::new();
        fs.negative_cache
          .lock()
          .get_or_insert_with(|| Box::new(super::NegativeDentryCache::new()))
          .insert("/created");
        fs.cache_insert("/created", 41);

        assert_eq!(fs.lookup_cache.lock().get("/created"), Some(&41));
        assert!(!fs.negative_cache.lock().as_ref().unwrap().contains("/created"));
    }
}
