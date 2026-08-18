//! 单文件页缓存条目、脏页写回和读写路径的实现。

use super::*;

/// 单文件逻辑大小与脏页索引（页号）。
// 本结构代码由AI完成
pub(crate) struct FileEntryInner {
    /// 文件的逻辑长度（字节）；可大于当前已落盘长度，直到脏页写回完成。
    pub(crate) logical_size : u64,
    /// 脏页号到修改版本号的映射，用于确认写回未与新写入竞争。
    pub(crate) dirty_pages : BTreeMap<u64, u64>,
    /// 上次 read 结束页号；用于顺序读检测后再预取（F-14）。
    pub(crate) last_read_end_page : Option<u64>,
}

/// 全局文件页缓存。
// 本结构代码由AI完成
pub struct GlobalFilePageCache {
    /// 当前缓存对应的挂载代次；Release 写入与 Acquire 读取保证切换后的元数据可见。
    mount_gen : AtomicU64,
    /// 帧池、索引和 LRU；持锁时不得执行下层 I/O。
    pub(crate) state : Mutex<GlobalCacheState>,
    /// 每文件元数据表；其锁必须先于单文件 RwLock 和 `state` 获取。
    pub(crate) files : Mutex<BTreeMap<FileCacheKey, Arc<RwLock<FileEntryInner>>>>,
    /// 仍被 [`PagedFileHandle`] 持有的路径数；归零时在 `close` 后回收该路径缓存条目。
    pub(crate) open_refs : Mutex<BTreeMap<FileCacheKey, usize>>,
}

impl GlobalFilePageCache {
    /// 构造与当前 `mount_gen` 绑定的缓存表。
// 本方法代码由AI完成
    pub fn new(mount_gen : u64) -> Self {
        Self { mount_gen : AtomicU64::new(mount_gen),
               state : Mutex::new(GlobalCacheState::new()),
               files : Mutex::new(BTreeMap::new()),
               open_refs : Mutex::new(BTreeMap::new()) }
    }

    pub fn mount_gen(&self) -> u64 { self.mount_gen
                                         .load(Ordering::Acquire) }

    /// 切换到新挂载代次：原地清空缓存元数据并复用已分配的帧池，
    /// 避免每次 mount/umount 重建 16MiB 缓存造成内核堆碎片化与长跑卡死。
    /// 仅应在已无活跃用户 fd 持有脏页的安全点调用（脏页须由 `flush_all` 先写回）。
// 本方法代码由AI完成
    pub fn reset_to_gen(&self, new_gen : u64) {
        self.state
            .lock()
            .clear_in_place();
        self.files
            .lock()
            .clear();
        self.open_refs
            .lock()
            .clear();
        self.mount_gen
            .store(new_gen, Ordering::Release);
    }

// 本方法代码由AI完成
    pub(crate) fn file_key(&self, path : &str) -> FileCacheKey {
        self.file_key_from_arc(Arc::from(path))
    }

    fn file_key_from_arc(&self, path : Arc<str>) -> FileCacheKey {
        FileCacheKey::path(self.mount_gen(), path)
    }

// 本方法代码由AI完成
    fn get_file_entry(&self, path : &str, initial_size : u64) -> Arc<RwLock<FileEntryInner>> {
        self.get_file_entry_for_key(&self.file_key(path), initial_size)
    }

    fn get_file_entry_for_key(&self,
                              key : &FileCacheKey,
                              initial_size : u64)
                              -> Arc<RwLock<FileEntryInner>> {
        let mut files = self.files.lock();
        if let Some(e) = files.get(&key) {
            let entry = e.clone();
            if initial_size > entry.read().logical_size {
                entry.write().logical_size = initial_size;
            }
            return entry;
        }
        let e = Arc::new(RwLock::new(FileEntryInner { logical_size : initial_size,
                                                      dirty_pages : BTreeMap::new(),
                                                      last_read_end_page : None }));
        files.insert(key.clone(), e.clone());
        e
    }

    /// 普通文件句柄 `open`/`dup` 时登记；与 VFS `close` 配对。
// 本方法代码由AI完成
    pub fn acquire_open_ref(&self, path : &str) {
        self.acquire_open_ref_key(&self.file_key(path));
    }

    pub fn acquire_open_ref_key(&self, key : &FileCacheKey) {
        let mut refs = self.open_refs.lock();
        *refs.entry(key.clone()).or_insert(0) += 1;
    }

    /// 句柄 `close` 后递减；最后一个引用消失时丢弃该路径的页缓存元数据（页帧已在 close 时 flush）。
// 本方法代码由AI完成
    pub fn release_open_ref(&self, path : &str) {
        self.release_open_ref_key(&self.file_key(path));
    }

    pub fn release_open_ref_key(&self, key : &FileCacheKey) {
        let should_purge = {
            let mut refs = self.open_refs.lock();
            let Some(count) = refs.get_mut(key) else {
                return;
            };
            // 关闭路径可因失败回滚或重复清理重入；饱和减法避免计数下溢成极大值并永久泄漏条目。
            *count = count.saturating_sub(1);
            if *count == 0 {
                refs.remove(key);
                true
            } else {
                false
            }
        };
        if should_purge {
            self.forget_closed_file_key(key);
        }
    }

    /// 最后一个句柄关闭时只移除路径元数据，缓存页继续由 LRU 保留。
    /// unlink/rename 路径仍调用 [`Self::purge_closed_file`] 强制清页。
    fn forget_closed_file_key(&self, key : &FileCacheKey) {
        self.files
            .lock()
            .remove(key);
    }

// 本方法代码由AI完成
    fn note_page_written_back(&self,
                              key : &FileCacheKey,
                              page_idx : u64,
                              version : u64) {
        let files = self.files.lock();
        if let Some(entry) = files.get(key) {
            let mut entry = entry.write();
            if entry.dirty_pages.get(&page_idx) == Some(&version) {
                entry.dirty_pages.remove(&page_idx);
            }
        }
    }

// 本方法代码由AI完成
    fn writeback_evicted_page<Io, E>(&self,
                                       io : &mut Io,
                                       key : &FileCacheKey,
                                       page_idx : u64,
                                       saved_data : &[u8],
                                       version : u64,
                                       logical_size : u64,
                                       map_err : fn(Io::Error) -> E)
                                       -> Result<(), E>
        where Io : PageCacheIo
    {
        let off = page_idx * FILE_PAGE_SIZE as u64;
        if off >= logical_size {
            self.note_page_written_back(key, page_idx, version);
            return Ok(());
        }
        let len = FILE_PAGE_SIZE.min(usize::try_from(logical_size - off).unwrap_or(0));
        io.write_range(key.path.as_ref(), off, &saved_data[..len])
          .map_err(map_err)?;
        self.note_page_written_back(key, page_idx, version);
        Ok(())
    }

// 本方法代码由AI完成
    fn logical_size_for_key(&self, key : &FileCacheKey, fallback : u64) -> u64 {
        let files = self.files.lock();
        files.get(key)
             .map(|entry| {
                 entry.read()
                      .logical_size
             })
             .unwrap_or(fallback)
    }


// 本方法代码由AI完成
    fn queue_flush_batch(batches : &mut Vec<(u64, Vec<u8>)>,
                         batch_start : &mut Option<u64>,
                         batch_data : &mut Vec<u8>) {
        if let Some(start_page) = batch_start.take() {
            if !batch_data.is_empty() {
                batches.push((start_page, core::mem::take(batch_data)));
            }
        }
    }

// 本方法代码由AI完成
    fn flush_dirty_run<Io, E>(&self,
                              io : &mut Io,
                              key : &FileCacheKey,
                              pages : &[(u64, u64)],
                              logical_size : u64,
                              map_err : fn(Io::Error) -> E)
                              -> Result<Vec<(u64, u64)>, E>
        where Io : PageCacheIo
    {
        let mut batches : Vec<(u64, Vec<u8>)> = Vec::new();
        let mut batch_start = None;
        let mut batch_last = None;
        let mut batch_data = Vec::with_capacity(pages.len()
                                                     .min(FLUSH_RUN_MAX_PAGES) *
                                                FILE_PAGE_SIZE);
        let mut flushed_pages = Vec::new();

        for &(page_idx, expected_version) in pages {
            let off = page_idx * FILE_PAGE_SIZE as u64;
            let mut slot = {
                let cache = self.state.lock();
                cache.index
                     .get(&(key.clone(), page_idx))
                     .copied()
            };
            if slot.is_none() && off < logical_size {
                self.install_page(io,
                                  key,
                                  page_idx,
                                  logical_size,
                                  InstallSource::Demand,
                                  map_err)?;
                slot = self.state.lock()
                                 .index
                                 .get(&(key.clone(), page_idx))
                                 .copied();
            }

            let cache = self.state.lock();
            let Some(slot) = slot else {
                if off >= logical_size {
                    flushed_pages.push((page_idx, expected_version));
                }
                continue;
            };
            let frame = &cache.frames[slot];
            let same_page = frame.key
                                 .as_ref()
                                 .map(|(frame_key, frame_page)| {
                                     frame_key == key && *frame_page == page_idx
                                 })
                                 .unwrap_or(false);
            if off >= logical_size ||
               !frame.dirty ||
               frame.version != expected_version ||
               !same_page
            {
                Self::queue_flush_batch(&mut batches,
                                        &mut batch_start,
                                        &mut batch_data);
                batch_last = None;
                flushed_pages.push((page_idx, expected_version));
                continue;
            }
            if batch_last.is_some_and(|last| last + 1 != page_idx) {
                Self::queue_flush_batch(&mut batches,
                                        &mut batch_start,
                                        &mut batch_data);
            }
            if batch_start.is_none() {
                batch_start = Some(page_idx);
            }

            let len = FILE_PAGE_SIZE.min(usize::try_from(logical_size - off).unwrap_or(0));
            batch_data.extend_from_slice(&cache.page_data(slot)[..len]);
            batch_last = Some(page_idx);
            flushed_pages.push((page_idx, expected_version));
        }
        Self::queue_flush_batch(&mut batches,
                                &mut batch_start,
                                &mut batch_data);

        for (start_page, data) in batches {
            let off = start_page * FILE_PAGE_SIZE as u64;
            io.write_range(key.path.as_ref(), off, &data)
              .map_err(map_err)?;
        }

        let mut cache = self.state.lock();
        for &(page_idx, version) in &flushed_pages {
            if let Some(&slot) = cache.index.get(&(key.clone(), page_idx)) {
                if cache.frames[slot].dirty && cache.frames[slot].version == version {
                    cache.mark_clean(slot);
                }
            }
        }

        Ok(flushed_pages)
    }

// 本方法代码由AI完成
    /// 将一个文件页装入帧池；若同页被并发装入，保留已有页而丢弃本次读取结果。
    ///
    /// 读盘和脏页回写均在释放 `state` 锁后进行。没有可用槽时仅短暂自旋等待锁外写回重新入队，
    /// 因此调用路径不得在禁止抢占的长临界区中调用本函数。
    fn install_page<Io, E>(&self,
                           io : &mut Io,
                           key : &FileCacheKey,
                           page_idx : u64,
                           file_size : u64,
                           _source : InstallSource,
                           map_err : fn(Io::Error) -> E)
                           -> Result<(), E>
        where Io : PageCacheIo
    {
        {
            let mut cache = self.state.lock();
            if cache.capacity == 0 {
                return Ok(());
            }
            let existing = cache.index
                                .get(&(key.clone(), page_idx))
                                .copied();
            #[cfg(feature = "diagnostics")]
            cache.note_lookup(existing, source);
            if let Some(idx) = existing {
                cache.touch_lru(idx);
                return Ok(());
            }
        }

        let page_off = page_idx * FILE_PAGE_SIZE as u64;
        let mut page_buf = [0u8; FILE_PAGE_SIZE];
        if page_off < file_size {
            let to_read = FILE_PAGE_SIZE.min(
                usize::try_from(file_size.saturating_sub(page_off)).unwrap_or(0),
            );
            if to_read > 0 {
                let n = io.read_range(key.path.as_ref(), page_off, &mut page_buf[..to_read])
                          .map_err(map_err)?;
                if n < to_read {
                    page_buf[n..to_read].fill(0);
                }
            }
        }

        // 第二次检查：锁外读盘期间可能已有其他路径装入该页。
        let mut cache = self.state.lock();
        if cache.capacity == 0 {
            return Ok(());
        }
        if let Some(&idx) = cache.index
                                 .get(&(key.clone(), page_idx))
        {
            #[cfg(feature = "diagnostics")]
            {
                cache.diagnostics.duplicate_loads += 1;
            }
            cache.touch_lru(idx);
            return Ok(());
        }

        loop {
            let Some(idx) = cache.pop_free_or_lru_index() else {
                drop(cache);
                core::hint::spin_loop();
                cache = self.state.lock();
                if let Some(&existing) = cache.index.get(&(key.clone(), page_idx)) {
                    cache.touch_lru(existing);
                    return Ok(());
                }
                continue;
            };
            let victim_was_dirty = cache.frames[idx].dirty;
            let dirty_victim = if cache.frames[idx].dirty {
                cache.frames[idx].key.clone().map(|(victim_key, victim_page)| {
                    (victim_key,
                     victim_page,
                     cache.page_data(idx).to_vec(),
                     cache.frames[idx].version)
                })
            } else {
                None
            };
            if let Some((victim_key, victim_page, saved_data, version)) = dirty_victim {
                drop(cache);
                let victim_logical = self.logical_size_for_key(
                    &victim_key,
                    victim_page.saturating_mul(FILE_PAGE_SIZE as u64) + FILE_PAGE_SIZE as u64,
                );
                let writeback = self.writeback_evicted_page(io,
                                                            &victim_key,
                                                            victim_page,
                                                            &saved_data,
                                                            version,
                                                            victim_logical,
                                                            map_err);
                cache = self.state.lock();
                let still_same = cache.frames[idx].dirty &&
                                 cache.frames[idx].version == version &&
                                 cache.frames[idx].key.as_ref() ==
                                 Some(&(victim_key.clone(), victim_page));
                if let Err(err) = writeback {
                    if still_same {
                        cache.touch_lru(idx);
                    }
                    return Err(err);
                }
                if !still_same {
                    if let Some(&existing) = cache.index.get(&(key.clone(), page_idx)) {
                        cache.touch_lru(existing);
                        return Ok(());
                    }
                    continue;
                }
                cache.mark_clean(idx);
            }
            let _ = cache.detach_slot_for_reuse(idx, victim_was_dirty);
            if let Some(&existing) = cache.index.get(&(key.clone(), page_idx)) {
                cache.return_detached_slot(idx);
                cache.touch_lru(existing);
                return Ok(());
            }
            cache.page_data_mut(idx)
                 .copy_from_slice(&page_buf);
            cache.frames[idx].dirty = false;
            cache.frames[idx].version = 0;
            cache.frames[idx].key = Some((key.clone(), page_idx));
            cache.index
                 .insert((key.clone(), page_idx), idx);
            #[cfg(feature = "diagnostics")]
            cache.note_install(idx, source);
            cache.touch_lru(idx);
            return Ok(());
        }
    }

// 本方法代码由AI完成
    /// 为超出 EOF 的写入准备全零页，避免为尚不存在的磁盘页执行无意义读取。
    fn install_zero_page<Io, E>(&self,
                                io : &mut Io,
                                key : &FileCacheKey,
                                page_idx : u64,
                                map_err : fn(Io::Error) -> E)
                                -> Result<(), E>
        where Io : PageCacheIo
    {
        {
            let mut cache = self.state.lock();
            if cache.capacity == 0 {
                return Ok(());
            }
            let existing = cache.index
                                .get(&(key.clone(), page_idx))
                                .copied();
            #[cfg(feature = "diagnostics")]
            cache.note_lookup(existing, InstallSource::Demand);
            if let Some(idx) = existing {
                cache.touch_lru(idx);
                return Ok(());
            }
        }

        let mut cache = self.state.lock();
        if cache.capacity == 0 {
            return Ok(());
        }
        if let Some(&idx) = cache.index
                                 .get(&(key.clone(), page_idx))
        {
            cache.touch_lru(idx);
            return Ok(());
        }

        loop {
            let Some(idx) = cache.pop_free_or_lru_index() else {
                drop(cache);
                core::hint::spin_loop();
                cache = self.state.lock();
                if let Some(&existing) = cache.index.get(&(key.clone(), page_idx)) {
                    cache.touch_lru(existing);
                    return Ok(());
                }
                continue;
            };
            let victim_was_dirty = cache.frames[idx].dirty;
            let dirty_victim = if cache.frames[idx].dirty {
                cache.frames[idx].key.clone().map(|(victim_key, victim_page)| {
                    (victim_key,
                     victim_page,
                     cache.page_data(idx).to_vec(),
                     cache.frames[idx].version)
                })
            } else {
                None
            };
            if let Some((victim_key, victim_page, saved_data, version)) = dirty_victim {
                drop(cache);
                let victim_logical = self.logical_size_for_key(
                    &victim_key,
                    victim_page.saturating_mul(FILE_PAGE_SIZE as u64) + FILE_PAGE_SIZE as u64,
                );
                let writeback = self.writeback_evicted_page(io,
                                                            &victim_key,
                                                            victim_page,
                                                            &saved_data,
                                                            version,
                                                            victim_logical,
                                                            map_err);
                cache = self.state.lock();
                let still_same = cache.frames[idx].dirty &&
                                 cache.frames[idx].version == version &&
                                 cache.frames[idx].key.as_ref() ==
                                 Some(&(victim_key.clone(), victim_page));
                if let Err(err) = writeback {
                    if still_same {
                        cache.touch_lru(idx);
                    }
                    return Err(err);
                }
                if !still_same {
                    if let Some(&existing) = cache.index.get(&(key.clone(), page_idx)) {
                        cache.touch_lru(existing);
                        return Ok(());
                    }
                    continue;
                }
                cache.mark_clean(idx);
            }
            let _ = cache.detach_slot_for_reuse(idx, victim_was_dirty);
            if let Some(&existing) = cache.index.get(&(key.clone(), page_idx)) {
                cache.return_detached_slot(idx);
                cache.touch_lru(existing);
                return Ok(());
            }
            cache.page_data_mut(idx)
                 .fill(0);
            cache.frames[idx].dirty = false;
            cache.frames[idx].version = 0;
            cache.frames[idx].key = Some((key.clone(), page_idx));
            cache.index
                 .insert((key.clone(), page_idx), idx);
            #[cfg(feature = "diagnostics")]
            cache.note_install(idx, InstallSource::Demand);
            cache.touch_lru(idx);
            return Ok(());
        }
    }

    /// Direct 模式：从 `offset` 读入 `buf`。
// 本方法代码由AI完成
    pub fn read<Io, E>(&self,
                       io : &mut Io,
                       path : &str,
                       file_size : u64,
                       offset : u64,
                       buf : &mut [u8],
                       map_err : fn(Io::Error) -> E)
                       -> Result<usize, E>
        where Io : PageCacheIo
    {
        self.read_key(io, &self.file_key(path), file_size, offset, buf, map_err)
    }

    pub fn read_key<Io, E>(&self,
                           io : &mut Io,
                           key : &FileCacheKey,
                           file_size : u64,
                           offset : u64,
                           buf : &mut [u8],
                           map_err : fn(Io::Error) -> E)
                           -> Result<usize, E>
        where Io : PageCacheIo
    {
        if buf.is_empty() || offset >= file_size {
            return Ok(0);
        }
        let entry = self.get_file_entry_for_key(key, file_size);
        let start_page = offset / FILE_PAGE_SIZE as u64;
        let sequential = entry.read()
                              .last_read_end_page
                              .map(|last| start_page == last.saturating_add(1))
                              .unwrap_or(false);
        let max = min(buf.len(),
                      usize::try_from(file_size - offset).unwrap_or(0));
        let mut done = 0usize;
        let mut pos = offset;
        while done < max {
            let page_idx = pos / FILE_PAGE_SIZE as u64;
            let page_off = (pos % FILE_PAGE_SIZE as u64) as usize;
            let chunk = (FILE_PAGE_SIZE - page_off).min(max - done);
            loop {
                self.install_page(io,
                                  key,
                                  page_idx,
                                  file_size,
                                  InstallSource::Demand,
                                  map_err)?;
                let cache = self.state.lock();
                let Some(&idx) = cache.index
                                      .get(&(key.clone(), page_idx))
                else {
                    // `install_page` 释放锁读盘后，淘汰或路径失效可抢先移除该页；重新安装后再复制。
                    continue;
                };
                buf[done..done + chunk].copy_from_slice(&cache.page_data(idx)
                                                             [page_off..page_off + chunk]);
                break;
            }
            done += chunk;
            pos += chunk as u64;
        }
        if done > 0 {
            let end_page = (offset + done as u64 - 1) / FILE_PAGE_SIZE as u64;
            entry.write().last_read_end_page = Some(end_page);
        }
        if sequential && FILE_READ_AHEAD_STRIDE > 0 {
            let prefetch_start = offset / FILE_PAGE_SIZE as u64 + 1;
            for ahead in 0..FILE_READ_AHEAD_STRIDE {
                let pi = prefetch_start + ahead as u64;
                if pi * FILE_PAGE_SIZE as u64 >= file_size {
                    break;
                }
                let _ = self.install_page(io,
                                          key,
                                          pi,
                                          file_size,
                                          InstallSource::Prefetch,
                                          map_err);
            }
        }
        Ok(done)
    }

    /// Direct 模式：从 `offset` 写入 `buf`。
// 本方法代码由AI完成
    pub fn write<Io, E>(&self,
                        io : &mut Io,
                        path : &str,
                        file_size : u64,
                        offset : u64,
                        buf : &[u8],
                        map_err : fn(Io::Error) -> E)
                        -> Result<usize, E>
        where Io : PageCacheIo
    {
        self.write_key(io, &self.file_key(path), file_size, offset, buf, map_err)
    }

    pub fn write_key<Io, E>(&self,
                            io : &mut Io,
                            key : &FileCacheKey,
                            file_size : u64,
                            offset : u64,
                            buf : &[u8],
                            map_err : fn(Io::Error) -> E)
                            -> Result<usize, E>
        where Io : PageCacheIo
    {
        if buf.is_empty() {
            return Ok(0);
        }

        let entry = self.get_file_entry_for_key(key, file_size);
        let mut pos = offset;
        let mut written = 0usize;
        while written < buf.len() {
            let page_idx = pos / FILE_PAGE_SIZE as u64;
            let page_off = (pos % FILE_PAGE_SIZE as u64) as usize;
            let chunk = (FILE_PAGE_SIZE - page_off).min(buf.len() - written);
            let page_start = page_idx * FILE_PAGE_SIZE as u64;
            let logical_size = {
                let guard = entry.read();
                guard.logical_size
            };
            loop {
                if page_start >= logical_size || (page_off == 0 && chunk == FILE_PAGE_SIZE) {
                    self.install_zero_page(io, key, page_idx, map_err)?;
                } else {
                    self.install_page(io,
                                      key,
                                      page_idx,
                                      logical_size,
                                      InstallSource::Demand,
                                      map_err)?;
                }
                // 在同一临界区发布页内容与文件长度，淘汰路径便不会按旧 EOF 丢弃刚扩展的写入。
                let mut guard = entry.write();
                let mut cache = self.state.lock();
                let Some(&idx) = cache.index
                                      .get(&(key.clone(), page_idx))
                else {
                    continue;
                };
                cache.page_data_mut(idx)[page_off..page_off + chunk].copy_from_slice(
                    &buf[written..written + chunk],
                );
                let version = cache.mark_dirty(idx);
                cache.touch_lru(idx);
                guard.dirty_pages
                     .insert(page_idx, version);
                let end = pos + chunk as u64;
                if end > guard.logical_size {
                    guard.logical_size = end;
                }
                break;
            }
            written += chunk;
            pos += chunk as u64;
        }
        Ok(written)
    }

// 本方法代码由AI完成
    /// 返回缓存记录的逻辑文件长度；不存在条目时使用调用者提供的下层长度。
    pub fn logical_size(&self, path : &str, fallback : u64) -> u64 {
        self.logical_size_key(&self.file_key(path), fallback)
    }

    pub fn logical_size_key(&self, key : &FileCacheKey, fallback : u64) -> u64 {
        self.logical_size_for_key(key, fallback)
    }

// 本方法代码由AI完成
    /// 设置逻辑长度；截断还应调用 [`Self::truncate`] 清除 EOF 后缓存页。
    pub fn set_logical_size(&self, path : &str, size : u64) {
        let entry = self.get_file_entry(path, size);
        entry.write()
             .logical_size = size;
    }

// 本方法代码由AI完成
    /// 返回指定文件尚待写回的页数，用于诊断和关闭前的同步判断。
    pub fn dirty_page_count(&self, path : &str) -> usize {
        let key = self.file_key(path);
        let files = self.files.lock();
        files.get(&key)
             .map(|entry| {
                 entry.read()
                      .dirty_pages
                      .len()
             })
             .unwrap_or(0)
    }

    /// 将脏页写回下层并清除脏标记。
// 本方法代码由AI完成
    pub fn flush<Io, E>(&self,
                        io : &mut Io,
                        path : &str,
                        map_err : fn(Io::Error) -> E)
                        -> Result<(), E>
        where Io : PageCacheIo
    {
        self.flush_key(io, &self.file_key(path), map_err)
    }

    pub fn flush_key<Io, E>(&self,
                            io : &mut Io,
                            key : &FileCacheKey,
                            map_err : fn(Io::Error) -> E)
                            -> Result<(), E>
        where Io : PageCacheIo
    {
        let entry = {
            let files = self.files.lock();
            files.get(key)
                 .cloned()
        };
        let Some(entry) = entry else {
            log::trace!("[page-cache-flush] key={:?} no-entry", key.path);
            return Ok(());
        };
        let (dirty, logical_size) = {
            let guard = entry.write();
            (guard.dirty_pages
                  .iter()
                  .map(|(&page, &version)| (page, version))
                  .collect::<Vec<_>>(),
             guard.logical_size)
        };

        let mut run = Vec::new();
        for (page_idx, version) in dirty {
            let should_flush = run.last()
                                  .is_some_and(|last : &(u64, u64)| last.0 + 1 != page_idx) ||
                               run.len() >= FLUSH_RUN_MAX_PAGES;
            if should_flush {
                let flushed = self.flush_dirty_run(io,
                                                   key,
                                                   &run,
                                                   logical_size,
                                                   map_err)?;
                {
                    let mut guard = entry.write();
                    for (flushed_page, flushed_version) in flushed {
                        if guard.dirty_pages.get(&flushed_page) == Some(&flushed_version) {
                            guard.dirty_pages.remove(&flushed_page);
                        }
                    }
                }
                run.clear();
            }
            run.push((page_idx, version));
        }
        if !run.is_empty() {
            let flushed = self.flush_dirty_run(io,
                                               key,
                                               &run,
                                               logical_size,
                                               map_err)?;
            let mut guard = entry.write();
            for (flushed_page, flushed_version) in flushed {
                if guard.dirty_pages.get(&flushed_page) == Some(&flushed_version) {
                    guard.dirty_pages.remove(&flushed_page);
                }
            }
        }
        Ok(())
    }

    /// 把当前缓存中所有文件的脏页写回下层。供整缓存回收（如测例脚本切换）前调用，
    /// 确保 [`reset_global_cache`] 丢弃旧缓存时不会丢失尚未落盘的数据。
// 本方法代码由AI完成
    pub fn flush_all<Io, E>(&self, io : &mut Io, map_err : fn(Io::Error) -> E) -> Result<(), E>
        where Io : PageCacheIo
    {
        let keys : Vec<FileCacheKey> = {
            let files = self.files.lock();
            files.keys()
                 .cloned()
                 .collect()
        };
        for key in keys {
            self.flush_key(io, &key, map_err)?;
        }
        Ok(())
    }

    /// 删除已关闭文件的缓存条目，释放 `dirty_pages`、`FileEntryInner` 和路径字符串的内存。
    /// 应在 VFS `close` 或 `unlink` 之后调用，防止 `files` BTreeMap 无限增长耗尽内核堆。
// 本方法代码由AI完成
    pub fn purge_closed_file(&self, path : &str) {
        let key = self.file_key(path);
        self.open_refs
            .lock()
            .remove(&key);
        self.files
            .lock()
            .remove(&key);

        let mut cache = self.state.lock();
        let keys_to_remove : Vec<(FileCacheKey, u64)> =
            cache.index
                 .range((key.clone(), 0)..=(key, u64::MAX))
                 .map(|(k, _)| k.clone())
                 .collect();
        for old in keys_to_remove {
            if let Some(slot) = cache.index
                                     .remove(&old)
            {
                cache.frames[slot].key = None;
                cache.frames[slot].dirty = false;
                cache.frames[slot].version = 0;
                cache.remove_from_lru(slot);
                if !cache.free
                         .iter()
                         .any(|&free_idx| free_idx == slot)
                {
                    cache.free
                         .push(slot);
                }
            }
        }
    }

    /// Rename 完成后丢弃两个旧 path-key 缓存，并把源对象的打开引用迁移到新路径。
    pub fn finish_rename(&self, old_path : &str, new_path : &str) {
        let old_key = self.file_key(old_path);
        let new_key = self.file_key(new_path);
        let source_refs = {
            let mut refs = self.open_refs.lock();
            let source_refs = refs.remove(&old_key).unwrap_or(0);
            refs.remove(&new_key);
            if source_refs != 0 {
                refs.insert(new_key.clone(), source_refs);
            }
            source_refs
        };
        self.files.lock().remove(&old_key);
        self.files.lock().remove(&new_key);

        let mut cache = self.state.lock();
        let mut keys_to_remove : Vec<(FileCacheKey, u64)> =
            cache.index
                 .range((old_key.clone(), 0)..=(old_key, u64::MAX))
                 .map(|(k, _)| k.clone())
                 .collect();
        keys_to_remove.extend(
            cache.index
                 .range((new_key.clone(), 0)..=(new_key, u64::MAX))
                 .map(|(k, _)| k.clone()),
        );
        for key in keys_to_remove {
            if let Some(slot) = cache.index.remove(&key) {
                cache.frames[slot].key = None;
                cache.frames[slot].dirty = false;
                cache.frames[slot].version = 0;
                cache.remove_from_lru(slot);
                if !cache.free.iter().any(|&free_idx| free_idx == slot) {
                    cache.free.push(slot);
                }
            }
        }
        log::trace!("[page-cache] rename refs={} old={} new={}",
                    source_refs,
                    old_path,
                    new_path);
    }

    /// 更新逻辑长度，并丢弃 EOF 之后的缓存页。
// 本方法代码由AI完成
    pub fn truncate(&self, path : &str, len : u64) {
        self.truncate_key(&self.file_key(path), len);
    }

    pub fn truncate_key(&self, key : &FileCacheKey, len : u64) {
        let entry = self.get_file_entry_for_key(key, len);
        {
            let mut guard = entry.write();
            guard.logical_size = len;
            let first_past_eof = if len == 0 {
                0
            } else {
                (len - 1) / FILE_PAGE_SIZE as u64 + 1
            };
            let to_remove : Vec<u64> = guard.dirty_pages
                                            .keys()
                                            .copied()
                                            .filter(|page_idx| *page_idx >= first_past_eof)
                                            .collect();
            for page_idx in to_remove {
                guard.dirty_pages
                     .remove(&page_idx);
            }
        }

        let mut cache = self.state.lock();
        let first_past_eof = if len == 0 {
            0
        } else {
            (len - 1) / FILE_PAGE_SIZE as u64 + 1
        };
        let keys_to_remove : Vec<(FileCacheKey, u64)> =
            cache.index
                 .range((key.clone(), first_past_eof)..=(key.clone(), u64::MAX))
                 .map(|(k, _)| k.clone())
                 .collect();
        for old in keys_to_remove {
            if let Some(slot) = cache.index
                                     .remove(&old)
            {
                cache.frames[slot].key = None;
                cache.frames[slot].dirty = false;
                cache.frames[slot].version = 0;
                cache.remove_from_lru(slot);
                cache.free
                     .push(slot);
            }
        }
        if len > 0 {
            let tail = (len % FILE_PAGE_SIZE as u64) as usize;
            if tail > 0 {
                let page_idx = len / FILE_PAGE_SIZE as u64;
                if let Some(&slot) = cache.index
                                          .get(&(key.clone(), page_idx))
                {
                    cache.page_data_mut(slot)[tail..].fill(0);
                }
            }
        }
    }
}

// 本变量代码由AI完成
