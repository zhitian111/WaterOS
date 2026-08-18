use super::*;

pub struct AnotherExt4Fs {
    /// 已加载的 ext4 后端；未挂载时为 `None`。
    pub(crate) fs : Option<Ext4>,
    /// 当前挂载使用的块设备句柄。
    pub(crate) device : Option<SharedBlockDevice>,
    /// 后端 I/O 错误标志，供后续操作快速拒绝。
    pub(crate) io_error_state : Option<Arc<AtomicBool>>,
    pub(crate) lookup_cache : Mutex<PositiveDentryCache>,
    pub(crate) negative_cache : Mutex<Option<Box<NegativeDentryCache>>>,
    pub(crate) open_nodes : BTreeMap<u32, usize>,
    pub(crate) orphan_nodes : BTreeMap<u32, String>,
    /// 用户可见 unlink 已提交但最终移除失败的隐藏链接；由 `sync` 和最终 close 重试。
    pub(crate) pending_reclaims : BTreeMap<u32, String>,
    pub(crate) orphan_dir : Option<u32>,
}

impl AnotherExt4Fs {
    pub(crate) const fn new() -> Self {
        Self { fs : None,
               device : None,
               io_error_state : None,
               lookup_cache : Mutex::new(PositiveDentryCache::new()),
               negative_cache : Mutex::new(None),
               open_nodes : BTreeMap::new(),
               orphan_nodes : BTreeMap::new(),
               pending_reclaims : BTreeMap::new(),
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
        self.fs
            .as_mut()
            .ok_or(FsError::NotMounted)
    }

    pub(crate) fn check_backend(&self) -> FsResult<()> { check_backend_error(&self.io_error_state) }

    pub(crate) fn lookup(&self, path : &str) -> FsResult<u32> {
        if let Some(inode) = self.lookup_cache
                                 .lock()
                                 .get(path)
        {
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
        let mut cache = self.lookup_cache
                            .lock();
        let evicted = cache.insert(path, inode, LOOKUP_CACHE_CAPACITY);
        lookup_diag_positive_evict(evicted);
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
        self.lookup_cache.lock().remove_subtree(path);
    }

    pub(crate) fn cache_rename_subtree(&self, old_path : &str, new_path : &str) {
        let evicted = self.lookup_cache
                          .lock()
                          .rename_subtree(old_path, new_path, LOOKUP_CACHE_CAPACITY);
        lookup_diag_positive_evict(evicted);
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
            fs.unlink(dir, name.as_str())
              .map_err(map_error)?;
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
            Err(FsError::NotFound) => (fs.mkdir(EXT4_ROOT_INO,
                                                OPEN_INODE_DIR.trim_start_matches('/'),
                                                InodeMode::DIRECTORY |
                                                InodeMode::from_bits_retain(0o700))
                                         .map_err(map_error)?,
                                       true),
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

    pub(crate) fn preserve_inode_for_unlink(&mut self, inode : u32) -> FsResult<()> {
        if self.orphan_nodes
               .contains_key(&inode)
        {
            return Ok(());
        }
        let dir = self.ensure_orphan_dir()?;
        let name = alloc::format!("{inode:08x}");
        self.get_mut()?
            .link(inode, dir, name.as_str())
            .map_err(map_error)?;
        self.get_mut()?
            .flush_all();
        let mut path = String::from(OPEN_INODE_DIR);
        path.push('/');
        path.push_str(name.as_str());
        self.orphan_nodes
            .insert(inode, name);
        self.cache_insert(path.as_str(), inode);
        Ok(())
    }

    /// 用户可见 unlink 成功后移除隐藏链接。失败会刻意延后，因为命名空间语义已经提交。
    pub(crate) fn reclaim_orphan(&mut self, inode : u32) {
        let Some(name) = self.orphan_nodes
                             .get(&inode)
                             .cloned()
        else {
            return;
        };
        let result = (|| -> FsResult<()> {
            let dir = self.orphan_dir
                          .ok_or(FsError::Io)?;
            self.get_mut()?
                .unlink(dir, name.as_str())
                .map_err(map_error)?;
            self.get_mut()?
                .flush_all();
            self.check_backend()
        })();
        match result {
            Ok(()) => {
                self.orphan_nodes
                    .remove(&inode);
                self.pending_reclaims
                    .remove(&inode);
            }
            Err(error) => {
                if self.pending_reclaims
                       .insert(inode, name)
                       .is_none()
                {
                    log::warn!("[fs::another-ext4] deferred reclaim inode={} failed: {:?}",
                               inode,
                               error);
                }
            }
        }
    }

    pub(crate) fn retry_pending_reclaims(&mut self) {
        let pending : Vec<u32> = self.pending_reclaims
                                     .keys()
                                     .copied()
                                     .collect();
        for inode in pending {
            self.reclaim_orphan(inode);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        check_backend_error, AnotherExt4Fs, AtomicBool, FsError, FsNodeId, Ordering, ReadWriteFs,
    };
    use alloc::boxed::Box;
    use alloc::sync::Arc;

    #[test]
    pub(crate) fn backend_error_latch_reports_io_after_failure() {
        let state = Some(Arc::new(AtomicBool::new(false)));
        assert_eq!(check_backend_error(&state), Ok(()));
        state.as_ref()
             .unwrap()
             .store(true, Ordering::Release);
        assert_eq!(check_backend_error(&state),
                   Err(FsError::Io));
    }

    #[test]
    pub(crate) fn lookup_cache_rename_moves_only_source_subtree() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/src", 10);
        fs.cache_insert("/src/child", 11);
        fs.cache_insert("/dst/stale", 12);
        fs.cache_insert("/unrelated", 13);
        {
            let mut negative = fs.negative_cache
                                 .lock();
            let negative =
                negative.get_or_insert_with(|| Box::new(super::NegativeDentryCache::new()));
            negative.insert("/dst");
            negative.insert("/dst/missing");
            negative.insert("/dstish/missing");
        }

        fs.cache_rename_subtree("/src", "/dst");

        let mut cache = fs.lookup_cache
                          .lock();
        assert_eq!(cache.get("/dst"), Some(10));
        assert_eq!(cache.get("/dst/child"), Some(11));
        assert_eq!(cache.get("/unrelated"), Some(13));
        assert!(!cache.contains("/src"));
        assert!(!cache.contains("/src/child"));
        assert!(!cache.contains("/dst/stale"));
        drop(cache);
        let negative = fs.negative_cache
                         .lock();
        let negative = negative.as_ref()
                               .unwrap();
        assert!(!negative.contains("/dst"));
        assert!(!negative.contains("/dst/missing"));
        assert!(negative.contains("/dstish/missing"));
    }

    #[test]
    pub(crate) fn stable_node_refcount_closes_exactly_once() {
        let mut fs = AnotherExt4Fs::new();
        fs.open_nodes
          .insert(42, 2);
        let node = FsNodeId::new(42);

        fs.close_node(node)
          .unwrap();
        assert_eq!(fs.open_nodes
                     .get(&42),
                   Some(&1));
        fs.close_node(node)
          .unwrap();
        assert!(!fs.open_nodes
                   .contains_key(&42));
        assert_eq!(fs.close_node(node),
                   Err(FsError::NotFound));
    }

    #[test]
    pub(crate) fn lookup_cache_remove_invalidates_descendants_only() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/tmp/work", 20);
        fs.cache_insert("/tmp/work/output", 21);
        fs.cache_insert("/tmp/worker", 22);

        fs.cache_remove_subtree("/tmp/work");

        let mut cache = fs.lookup_cache
                          .lock();
        assert!(!cache.contains("/tmp/work"));
        assert!(!cache.contains("/tmp/work/output"));
        assert_eq!(cache.get("/tmp/worker"), Some(22));
    }

    #[test]
    pub(crate) fn negative_cache_requires_full_path_match_and_removes_exact_entry() {
        let mut cache = super::NegativeDentryCache::new();
        let original = "/missing/a";
        let bucket = super::NegativeDentryCache::bucket(super::negative_path_hash(original));
        let collision = (0..10_000).map(|index| alloc::format!("/collision/{index}"))
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
        assert_eq!(cache.remove_exact(collision.as_str()),
                   0);
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

        assert_eq!(fs.lookup_cache.lock().get("/created"), Some(41));
        assert!(!fs.negative_cache
                   .lock()
                   .as_ref()
                   .unwrap()
                   .contains("/created"));
    }
}
