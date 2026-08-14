use super::*;

pub(crate) struct StableNodeLease {
    pub(crate) fs : SharedRwFs,
    pub(crate) node : FsNodeId,
    pub(crate) identity : MountIdentity,
    pub(crate) content_identity : VfsFileContentIdentity,
}

impl StableNodeLease {
    pub(crate) fn cache_key(&self) -> String {
        alloc::format!("@node:{:016x}:{:016x}", self.identity.mount_id, self.node.raw())
    }

    pub(crate) fn metadata(&self) -> VfsResult<VfsMetadata> {
        self.fs.lock()
               .metadata_node(self.node)
               .map(|meta| crate::map_meta(meta, self.identity))
               .map_err(map_fs_err)
    }

    pub(crate) fn read_range(&self, offset : u64, buf : &mut [u8]) -> VfsResult<usize> {
        self.fs.lock()
               .read_range_node(self.node, offset, buf)
               .map_err(map_fs_err)
    }

    pub(crate) fn write_range(&self, offset : u64, data : &[u8]) -> VfsResult<usize> {
        let mut done = 0usize;
        while done < data.len() {
            let written = self.fs.lock()
                                  .write_range_node(self.node,
                                                    offset + done as u64,
                                                    &data[done..])
                                  .map_err(map_fs_err)?;
            if written == 0 {
                return Err(VfsError::Io);
            }
            done = done.checked_add(written).ok_or(VfsError::Io)?;
        }
        Ok(done)
    }

    pub(crate) fn truncate(&self, len : u64) -> VfsResult<()> {
        self.fs.lock().truncate_node(self.node, len).map_err(map_fs_err)
    }

    pub(crate) fn sync(&self) -> VfsResult<()> { self.fs.lock().sync().map_err(map_fs_err) }

    pub(crate) fn mark_content_changed(&self) { self.content_identity.mark_changed(); }

    pub(crate) fn link_tmpfile(&self, new_path : &str) -> VfsResult<()> {
        let (fs, rel, identity) = match resolve_route(new_path)? {
            FsRoute::Root { abs, identity } => (root_rw()?, abs, identity),
            FsRoute::AuxRw { fs, rel, identity, .. } => (fs, rel, identity),
            FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } |
            FsRoute::PseudoSecurity { .. } => return Err(VfsError::ReadOnlyFs),
        };
        if identity.mount_id != self.identity.mount_id || !Arc::ptr_eq(&fs, &self.fs) {
            return Err(VfsError::Unsupported);
        }
        fs.lock().link_node(self.node, rel.as_str()).map_err(map_fs_err)?;
        self.mark_content_changed();
        Ok(())
    }
}

impl Drop for StableNodeLease {
    fn drop(&mut self) {
        if let Err(error) = self.fs.lock().close_node(self.node) {
            log::warn!("[paged_handle] stable node close failed node={} mount={} err={error:?}",
                       self.node.raw(),
                       self.identity.mount_id);
        }
    }
}

type StableContentKey = (u64, u64, u64);
static CONTENT_VERSIONS : Mutex<BTreeMap<StableContentKey, Weak<AtomicU64>>> =
    Mutex::new(BTreeMap::new());

fn stable_content_identity(mount_gen : u64,
                           identity : MountIdentity,
                           node : FsNodeId)
                           -> VfsFileContentIdentity {
    let key = (mount_gen, identity.mount_id, node.raw());
    let mut versions = CONTENT_VERSIONS.lock();
    let version = versions.get(&key)
                          .and_then(Weak::upgrade)
                          .unwrap_or_else(|| {
                              let version = Arc::new(AtomicU64::new(1));
                              versions.insert(key, Arc::downgrade(&version));
                              version
                          });
    VfsFileContentIdentity::new(mount_gen,
                                identity.mount_id,
                                node.raw(),
                                version)
}

pub(crate) fn open_stable_node(mount_gen : u64, path : &str) -> VfsResult<Option<Arc<StableNodeLease>>> {
    let (fs, rel, identity) = match resolve_route(path)? {
        FsRoute::Root { abs, identity } => (root_rw()?, abs, identity),
        FsRoute::AuxRw { fs, rel, identity, .. } => (fs, rel, identity),
        FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } | FsRoute::PseudoSecurity { .. } => {
            return Ok(None);
        }
    };
    let node = match fs.lock().open_node(rel.as_str()) {
        Ok(node) => node,
        // Stable node handles only regular files. A backend may report
        // `NotAFile` for directories and symlinks; that is not a path error
        // for callers such as unlink, which must still reach the backend.
        Err(FsError::Unsupported | FsError::NotAFile) => return Ok(None),
        Err(error) => return Err(map_fs_err(error)),
    };
    let content_identity = stable_content_identity(mount_gen, identity, node);
    Ok(Some(Arc::new(StableNodeLease { fs,
                                      node,
                                      identity,
                                      content_identity })))
}

pub(crate) fn create_tmpfile_stable(
    mount_gen : u64,
    directory : &str,
    mode : u32,
    uid : u32,
    gid : u32,
) -> VfsResult<Arc<StableNodeLease>> {
    let (fs, rel, identity) = match resolve_route(directory)? {
        FsRoute::Root { abs, identity } => (root_rw()?, abs, identity),
        FsRoute::AuxRw { fs, rel, identity, .. } => (fs, rel, identity),
        FsRoute::AuxRo { .. } | FsRoute::PseudoProc { .. } |
        FsRoute::PseudoSecurity { .. } => return Err(VfsError::ReadOnlyFs),
    };
    let node = fs.lock()
                 .create_tmpfile_node(rel.as_str(), mode, uid, gid)
                 .map_err(map_fs_err)?;
    let content_identity = stable_content_identity(mount_gen, identity, node);
    Ok(Arc::new(StableNodeLease { fs, node, identity, content_identity }))
}

pub(crate) struct DetachedState {
    pub(crate) detached : bool,
    pub(crate) path : String,
    pub(crate) data : Vec<u8>,
    pub(crate) stable : Option<Arc<StableNodeLease>>,
    pub(crate) cache_key : String,
}

pub(crate) fn file_key_for_state(mount_gen : u64, state : &DetachedState) -> FileCacheKey {
    let path = Arc::from(state.cache_key.as_str());
    match state.stable.as_ref() {
        Some(node) => FileCacheKey::stable(mount_gen,
                                           path,
                                           node.identity.mount_id,
                                           node.node.raw()),
        None => FileCacheKey::path(mount_gen, path),
    }
}

type DetachedKey = (u64, String);
static DETACHED_STATES : Mutex<BTreeMap<DetachedKey, Weak<Mutex<DetachedState>>>> =
    Mutex::new(BTreeMap::new());
type StableNodeKey = (u64, String);
static STABLE_NODES : Mutex<BTreeMap<StableNodeKey, Weak<StableNodeLease>>> =
    Mutex::new(BTreeMap::new());
static STABLE_NODE_REGISTRATIONS : AtomicU64 = AtomicU64::new(0);

fn register_stable_node(mount_gen : u64, stable : &Arc<StableNodeLease>) {
    let mut nodes = STABLE_NODES.lock();
    if STABLE_NODE_REGISTRATIONS.fetch_add(1, Ordering::Relaxed) & 0xff == 0 {
        nodes.retain(|_, node| node.strong_count() != 0);
    }
    nodes.insert((mount_gen, stable.cache_key()), Arc::downgrade(stable));
}

pub(crate) fn stable_node_for_cache_key(mount_gen : u64,
                             cache_key : &str)
                             -> Option<Arc<StableNodeLease>> {
    let key = (mount_gen, String::from(cache_key));
    let mut nodes = STABLE_NODES.lock();
    let stable = nodes.get(&key).and_then(Weak::upgrade);
    if stable.is_none() {
        nodes.remove(&key);
    }
    stable
}

pub(crate) fn detached_state_for_open(mount_gen : u64,
                           path : &str,
                           stable : Option<Arc<StableNodeLease>>)
                           -> Arc<Mutex<DetachedState>> {
    let key = (mount_gen, String::from(path));
    let mut states = DETACHED_STATES.lock();
    if let Some(state) = states.get(&key).and_then(Weak::upgrade) {
        if let Some(stable) = state.lock().stable.as_ref() {
            register_stable_node(mount_gen, stable);
        }
        return state;
    }
    let cache_key = stable.as_ref()
                          .map(|node| node.cache_key())
                          .unwrap_or_else(|| String::from(path));
    let state = Arc::new(Mutex::new(DetachedState { detached : false,
                                                    path : String::from(path),
                                                    data : Vec::new(),
                                                    stable,
                                                    cache_key }));
    if let Some(stable) = state.lock().stable.as_ref() {
        register_stable_node(mount_gen, stable);
    }
    states.insert(key, Arc::downgrade(&state));
    state
}

pub(crate) struct PendingUnlinkDetach {
    key : DetachedKey,
    state : Arc<Mutex<DetachedState>>,
    data : Option<Vec<u8>>,
}

impl PendingUnlinkDetach {
    pub(crate) fn commit(self) {
        DETACHED_STATES.lock().remove(&self.key);
        let mut state = self.state.lock();
        if let Some(data) = self.data {
            state.data = data;
            state.detached = true;
        }
    }
}

pub(crate) fn commit_rename_state(old_path : &str,
                                  new_path : &str,
                                  replaced : Option<PendingUnlinkDetach>) {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    let old_key = (mount_gen, String::from(old_path));
    let new_key = (mount_gen, String::from(new_path));
    let mut states = DETACHED_STATES.lock();
    let source = states.remove(&old_key).and_then(|state| state.upgrade());
    states.remove(&new_key);

    let (target_state, mut target_data) = match replaced {
        Some(replaced) => (Some(replaced.state), Some(replaced.data)),
        None => (None, None),
    };
    let mut target = target_state.as_ref().map(|state| state.lock());
    let mut source_state = source.as_ref().map(|source| source.lock());
    global_cache(mount_gen).finish_rename(old_path, new_path);
    if let (Some(target), Some(Some(data))) = (target.as_mut(), target_data.take()) {
        target.data = data;
        target.detached = true;
    }
    if let (Some(source), Some(source_state)) = (source.as_ref(), source_state.as_mut()) {
        source_state.path = String::from(new_path);
        if source_state.stable.is_none() {
            source_state.cache_key = String::from(new_path);
        }
        states.insert(new_key, Arc::downgrade(&source));
    }
}

pub(crate) fn prepare_unlink_detach(path : &str) -> VfsResult<Option<PendingUnlinkDetach>> {
    let mount_gen = fs::rootfs::active_impl::mount_generation();
    let key = (mount_gen, String::from(path));
    let state = {
        let mut states = DETACHED_STATES.lock();
        let state = states.get(&key).and_then(Weak::upgrade);
        if state.is_none() {
            states.remove(&key);
        }
        state
    };
    let Some(state) = state else {
        if let Some(stable) = open_stable_node(mount_gen, path)? {
            stable.mark_content_changed();
        }
        return Ok(None);
    };
    if state.lock().detached {
        return Ok(None);
    }

    let (meta, cache_key, stable) = {
        let state = state.lock();
        let meta = match state.stable.as_ref() {
            Some(node) => node.metadata()?,
            None => FsBridge.metadata(path)?,
        };
        (meta, state.cache_key.clone(), state.stable.clone())
    };
    if let Some(stable) = stable.as_ref() {
        stable.mark_content_changed();
        return Ok(Some(PendingUnlinkDetach { key, state, data : None }));
    }
    let cache = global_cache(mount_gen);
    let logical_size = cache.logical_size(cache_key.as_str(), meta.size);
    let len = usize::try_from(logical_size).map_err(|_| VfsError::Io)?;
    check_detached_len(len)?;
    let mut data = try_zeroed(len)?;
    if len != 0 {
        let mut io = FsPageIo::new(mount_gen, stable);
        let read = cache.read(&mut io,
                              cache_key.as_str(),
                              logical_size,
                              0,
                              &mut data,
                              core::convert::identity)?;
        if read != len {
            return Err(VfsError::Io);
        }
    }
    Ok(Some(PendingUnlinkDetach { key, state, data : Some(data) }))
}

// 本方法代码由AI完成
fn check_detached_len(len : usize) -> VfsResult<()> {
    if len > DETACHED_DATA_MAX {
        log::warn!("[paged_handle] detached buffer cap exceeded len={} max={}",
                   len,
                   DETACHED_DATA_MAX);
        return Err(VfsError::Io);
    }
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn grow_detached_data(buf : &mut Vec<u8>, new_len : usize) -> VfsResult<()> {
    check_detached_len(new_len)?;
    if buf.len() < new_len {
        buf.try_reserve_exact(new_len - buf.len())
           .map_err(|_| VfsError::Io)?;
        buf.resize(new_len, 0);
    }
    Ok(())
}
