//! 全局共享文件页缓存（Direct 模式）：按 `(mount_gen, path, page_index)` 键 LRU 缓存页帧。
//!
//! 每个文件条目使用 [`Arc<RwLock<FileEntryInner>>`]：允许多个读者并发，写/刷盘独占。
//!
//! ## Lock ordering
//!
//! 与 `wateros-vfs-impl-fs-bridge` 的 `paged_handle` 约定一致，编号越小越先获取，禁止逆序：
//!
//! 1. `files`（`Mutex`，极短）
//! 2. per-file `FileEntryInner` RwLock（`read` / `write`）
//! 3. `state`（`Mutex`，极短；**持锁期间不得调用下层块设备 I/O**）
//! 4. 根卷 `SharedRwFs`（仅在 [`PageCacheIo`] 的 `read_range` / `write_range` 内短持有）
//!
//! `install_page` / `install_zero_page` 在调 I/O 前会 `drop(state)`；调用方须在持有
//! entry 锁后再通过 `PageCacheIo` 下探 ext4，不得在持有 ext4 锁后再等 entry 锁。
//! 本模块代码由AI完成

#![no_std]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::min;
use core::hash::{Hash, Hasher};
use core::sync::atomic::{AtomicU64, Ordering};
use spin::{Mutex, RwLock};
use wateros_base_config::fs::{FILE_PAGE_CACHE_CAPACITY, FILE_PAGE_SIZE, FILE_READ_AHEAD_STRIDE};

// 本变量代码由AI完成
const FLUSH_RUN_MAX_PAGES : usize = 64;

/// 区间读写下层（通常由 `FsBridge` 委托 `ReadOnlyFs` / `ReadWriteFs`）。
pub trait PageCacheIo {
    type Error;
// 本方法代码由AI完成
    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, Self::Error>;
// 本方法代码由AI完成
    fn write_range(&mut self,
                   path : &str,
                   offset : u64,
                   data : &[u8])
                   -> Result<usize, Self::Error>;
}

/// 页缓存键：根卷挂载代次 + 绝对路径；可带稳定文件 node id 加速 BTree 比较。
#[derive(Clone, Debug)]
// 本结构代码由AI完成
pub struct FileCacheKey {
    pub mount_gen : u64,
    /// 稳定文件的 `(mount_id, node_id)`；`None` 表示没有稳定 node 的路径键。
    pub stable : Option<(u64, u64)>,
    pub path : Arc<str>,
}

impl FileCacheKey {
    pub fn path(mount_gen : u64, path : Arc<str>) -> Self {
        Self { mount_gen,
               stable : None,
               path }
    }

    pub fn stable(mount_gen : u64, path : Arc<str>, mount_id : u64, node_id : u64) -> Self {
        Self { mount_gen,
               stable : Some((mount_id, node_id)),
               path }
    }
}

impl PartialEq for FileCacheKey {
    fn eq(&self, other : &Self) -> bool {
        if self.mount_gen != other.mount_gen {
            return false;
        }
        match (self.stable, other.stable) {
            (Some(left), Some(right)) => left == right,
            (None, None) => self.path == other.path,
            _ => false,
        }
    }
}

impl Eq for FileCacheKey {}

impl PartialOrd for FileCacheKey {
    fn partial_cmp(&self, other : &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileCacheKey {
    fn cmp(&self, other : &Self) -> core::cmp::Ordering {
        self.mount_gen
            .cmp(&other.mount_gen)
            .then_with(|| match (self.stable, other.stable) {
                (Some(left), Some(right)) => left.cmp(&right),
                (None, None) => self.path.cmp(&other.path),
                (None, Some(_)) => core::cmp::Ordering::Less,
                (Some(_), None) => core::cmp::Ordering::Greater,
            })
    }
}

impl Hash for FileCacheKey {
    fn hash<H : Hasher>(&self, state : &mut H) {
        self.mount_gen.hash(state);
        match self.stable {
            Some(stable) => {
                1u8.hash(state);
                stable.hash(state);
            }
            None => {
                0u8.hash(state);
                self.path.hash(state);
            }
        }
    }
}

// 本结构代码由AI完成
struct PageFrame {
    key : Option<(FileCacheKey, u64)>,
    dirty : bool,
    version : u64,
    lru_prev : Option<usize>,
    lru_next : Option<usize>,
    lru_class : Option<LruClass>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LruClass {
    Clean,
    Dirty,
}

// 本结构代码由AI完成
struct GlobalCacheState {
    capacity : usize,
    /// 所有页帧 payload 共用连续池，避免每槽一次 4 KiB 堆分配放大 TLSF 锁竞争与碎片。
    data : Vec<u8>,
    frames : Vec<PageFrame>,
    index : BTreeMap<(FileCacheKey, u64), usize>,
    clean_lru_head : Option<usize>,
    clean_lru_tail : Option<usize>,
    dirty_lru_head : Option<usize>,
    dirty_lru_tail : Option<usize>,
    free : Vec<usize>,
    next_version : u64,
}

impl GlobalCacheState {
// 本方法代码由AI完成
    fn new() -> Self { Self::with_capacity(FILE_PAGE_CACHE_CAPACITY) }

    fn with_capacity(cap : usize) -> Self {
        let mut frames = Vec::new();
        let mut free = Vec::new();
        let data = vec![0u8; cap.checked_mul(FILE_PAGE_SIZE).unwrap_or(usize::MAX)];
        if cap > 0 {
            frames.reserve_exact(cap);
            for _ in 0..cap {
                frames.push(PageFrame { key : None,
                                        dirty : false,
                                        version : 0,
                                        lru_prev : None,
                                        lru_next : None,
                                        lru_class : None });
            }
            free.extend((0..cap).rev());
        }
        Self { capacity : cap,
               data,
               frames,
               index : BTreeMap::new(),
               clean_lru_head : None,
               clean_lru_tail : None,
               dirty_lru_head : None,
               dirty_lru_tail : None,
               free,
               next_version : 0 }
    }

    #[inline]
    fn page_data(&self, idx : usize) -> &[u8] {
        let start = idx * FILE_PAGE_SIZE;
        &self.data[start..start + FILE_PAGE_SIZE]
    }

    #[inline]
    fn page_data_mut(&mut self, idx : usize) -> &mut [u8] {
        let start = idx * FILE_PAGE_SIZE;
        &mut self.data[start..start + FILE_PAGE_SIZE]
    }

    fn lru_ends(&self, class : LruClass) -> (Option<usize>, Option<usize>) {
        match class {
            LruClass::Clean => (self.clean_lru_head, self.clean_lru_tail),
            LruClass::Dirty => (self.dirty_lru_head, self.dirty_lru_tail),
        }
    }

    fn set_lru_head(&mut self, class : LruClass, head : Option<usize>) {
        match class {
            LruClass::Clean => self.clean_lru_head = head,
            LruClass::Dirty => self.dirty_lru_head = head,
        }
    }

    fn set_lru_tail(&mut self, class : LruClass, tail : Option<usize>) {
        match class {
            LruClass::Clean => self.clean_lru_tail = tail,
            LruClass::Dirty => self.dirty_lru_tail = tail,
        }
    }

    fn remove_from_lru(&mut self, idx : usize) {
        let (class, prev, next) = {
            let frame = &mut self.frames[idx];
            let Some(class) = frame.lru_class.take() else {
                return;
            };
            let prev = frame.lru_prev.take();
            let next = frame.lru_next.take();
            (class, prev, next)
        };
        if let Some(prev) = prev {
            self.frames[prev].lru_next = next;
        } else {
            self.set_lru_head(class, next);
        }
        if let Some(next) = next {
            self.frames[next].lru_prev = prev;
        } else {
            self.set_lru_tail(class, prev);
        }
    }

    fn push_lru_back(&mut self, idx : usize, class : LruClass) {
        debug_assert!(self.frames[idx].lru_class.is_none());
        let (_, tail) = self.lru_ends(class);
        self.frames[idx].lru_prev = tail;
        self.frames[idx].lru_next = None;
        self.frames[idx].lru_class = Some(class);
        if let Some(tail) = tail {
            self.frames[tail].lru_next = Some(idx);
        } else {
            self.set_lru_head(class, Some(idx));
        }
        self.set_lru_tail(class, Some(idx));
    }

// 本方法代码由AI完成
    fn touch_lru(&mut self, idx : usize) {
        let class = if self.frames[idx].dirty {
            LruClass::Dirty
        } else {
            LruClass::Clean
        };
        if self.frames[idx].lru_class == Some(class) &&
           self.lru_ends(class).1 == Some(idx)
        {
            return;
        }
        self.remove_from_lru(idx);
        self.push_lru_back(idx, class);
    }

    fn pop_lru_front(&mut self, class : LruClass) -> Option<usize> {
        let (head, _) = self.lru_ends(class);
        if let Some(idx) = head {
            self.remove_from_lru(idx);
        }
        head
    }

// 本方法代码由AI完成
    fn pop_free_or_lru_index(&mut self) -> Option<usize> {
        if let Some(idx) = self.free.pop() {
            return Some(idx);
        }
        // Keep clean and dirty slots in separate intrusive LRUs. A miss can
        // discard the oldest clean page in O(1) without making an unrelated
        // temporary-file writeback part of an executable-page read.
        if let Some(idx) = self.pop_lru_front(LruClass::Clean) {
            return Some(idx);
        }
        if let Some(idx) = self.pop_lru_front(LruClass::Dirty) {
            return Some(idx);
        }
        // 所有槽位都可能正在锁外写回。调用方等待其重新进入 LRU，不能绕过
        // dirty/version 协议强制清理 index 中的任意槽位。
        None
    }

// 本方法代码由AI完成
    fn detach_slot_for_reuse(&mut self,
                             idx : usize)
                             -> Option<((FileCacheKey, u64), Vec<u8>, u64)> {
        let old = self.frames[idx].key.take();
        if let Some(ref key) = old {
            self.index.remove(key);
        }
        self.remove_from_lru(idx);
        let dirty_data = if self.frames[idx].dirty {
            old.clone()
               .map(|key| (key, self.page_data(idx).to_vec(), self.frames[idx].version))
        } else {
            None
        };
        self.frames[idx].dirty = false;
        dirty_data
    }

    fn mark_dirty(&mut self, idx : usize) -> u64 {
        self.remove_from_lru(idx);
        self.next_version = self.next_version.wrapping_add(1);
        if self.next_version == 0 {
            self.next_version = 1;
        }
        self.frames[idx].dirty = true;
        self.frames[idx].version = self.next_version;
        self.push_lru_back(idx, LruClass::Dirty);
        self.next_version
    }

    fn mark_clean(&mut self, idx : usize) {
        if !self.frames[idx].dirty {
            return;
        }
        self.remove_from_lru(idx);
        self.frames[idx].dirty = false;
        if self.frames[idx].key.is_some() {
            self.push_lru_back(idx, LruClass::Clean);
        }
    }

// 本方法代码由AI完成
    fn return_detached_slot(&mut self, idx : usize) {
        if self.frames[idx].key.is_none() &&
           !self.free
                .iter()
                .any(|&free_idx| free_idx == idx)
        {
            self.free.push(idx);
        }
    }

    /// 原地清空所有帧元数据并复用已分配的页帧内存（不释放/重分配 16MiB 帧池）。
    /// 供挂载代次切换时调用，避免每次 mount/umount 都重建整个缓存导致内核堆碎片化。
// 本方法代码由AI完成
    fn clear_in_place(&mut self) {
        for frame in self.frames
                         .iter_mut()
        {
            frame.key = None;
            frame.dirty = false;
            frame.version = 0;
            frame.lru_prev = None;
            frame.lru_next = None;
            frame.lru_class = None;
        }
        self.index
            .clear();
        self.clean_lru_head = None;
        self.clean_lru_tail = None;
        self.dirty_lru_head = None;
        self.dirty_lru_tail = None;
        self.free
            .clear();
        self.free
            .extend((0..self.capacity).rev());
    }

    #[cfg(test)]
    fn assert_lru_invariants(&self) {
        let mut seen = vec![false; self.capacity];
        for class in [LruClass::Clean, LruClass::Dirty] {
            let (head, tail) = self.lru_ends(class);
            let mut cursor = head;
            let mut previous = None;
            let mut count = 0usize;
            while let Some(idx) = cursor {
                assert!(idx < self.capacity);
                assert!(!seen[idx], "slot {idx} appears in more than one LRU position");
                seen[idx] = true;
                let frame = &self.frames[idx];
                assert_eq!(frame.lru_class, Some(class));
                assert_eq!(frame.lru_prev, previous);
                assert_eq!(frame.dirty, class == LruClass::Dirty);
                assert!(frame.key.is_some());
                previous = Some(idx);
                cursor = frame.lru_next;
                count += 1;
                assert!(count <= self.capacity, "LRU cycle detected");
            }
            assert_eq!(previous, tail);
            assert_eq!(head.is_none(), tail.is_none());
        }

        let mut free_seen = vec![false; self.capacity];
        for &idx in &self.free {
            assert!(idx < self.capacity);
            assert!(!free_seen[idx], "duplicate free slot {idx}");
            free_seen[idx] = true;
            assert!(!seen[idx], "slot {idx} is both free and active");
            assert!(self.frames[idx].key.is_none());
            assert!(self.frames[idx].lru_class.is_none());
        }

        for (idx, frame) in self.frames.iter().enumerate() {
            match &frame.key {
                Some(key) => {
                    assert_eq!(self.index.get(key), Some(&idx));
                    assert!(seen[idx], "active slot {idx} is missing from LRU");
                }
                None => {
                    assert!(!seen[idx], "detached slot {idx} remains in LRU");
                    assert!(free_seen[idx], "stable detached slot {idx} is not free");
                }
            }
        }
        assert_eq!(self.index.len(), seen.iter().filter(|seen| **seen).count());
    }
}

/// 单文件逻辑大小与脏页索引（页号）。
// 本结构代码由AI完成
struct FileEntryInner {
    logical_size : u64,
    dirty_pages : BTreeMap<u64, u64>,
    /// 上次 read 结束页号；用于顺序读检测后再预取（F-14）。
    last_read_end_page : Option<u64>,
}

/// 全局文件页缓存。
// 本结构代码由AI完成
pub struct GlobalFilePageCache {
    mount_gen : AtomicU64,
    state : Mutex<GlobalCacheState>,
    files : Mutex<BTreeMap<FileCacheKey, Arc<RwLock<FileEntryInner>>>>,
    /// 仍被 [`PagedFileHandle`] 持有的路径数；归零时在 `close` 后回收该路径缓存条目。
    open_refs : Mutex<BTreeMap<FileCacheKey, usize>>,
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
    fn file_key(&self, path : &str) -> FileCacheKey {
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
                self.install_page(io, key, page_idx, logical_size, map_err)?;
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
    fn install_page<Io, E>(&self,
                           io : &mut Io,
                           key : &FileCacheKey,
                           page_idx : u64,
                           file_size : u64,
                           map_err : fn(Io::Error) -> E)
                           -> Result<(), E>
        where Io : PageCacheIo
    {
        {
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
            let _ = cache.detach_slot_for_reuse(idx);
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
            cache.touch_lru(idx);
            return Ok(());
        }
    }

// 本方法代码由AI完成
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
            if let Some(&idx) = cache.index
                                     .get(&(key.clone(), page_idx))
            {
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
            let _ = cache.detach_slot_for_reuse(idx);
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
                self.install_page(io, key, page_idx, file_size, map_err)?;
                let cache = self.state.lock();
                let Some(&idx) = cache.index
                                      .get(&(key.clone(), page_idx))
                else {
                    // Eviction or path invalidation may race between
                    // install_page dropping the cache lock and this lookup.
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
                let _ = self.install_page(io, key, pi, file_size, map_err);
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
                                      map_err)?;
                }
                // Publish frame data and file metadata together so eviction
                // cannot discard an extending write using the old EOF.
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
    pub fn logical_size(&self, path : &str, fallback : u64) -> u64 {
        self.logical_size_key(&self.file_key(path), fallback)
    }

    pub fn logical_size_key(&self, key : &FileCacheKey, fallback : u64) -> u64 {
        self.logical_size_for_key(key, fallback)
    }

// 本方法代码由AI完成
    pub fn set_logical_size(&self, path : &str, size : u64) {
        let entry = self.get_file_entry(path, size);
        entry.write()
             .logical_size = size;
    }

// 本方法代码由AI完成
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
static GLOBAL_CACHE : Mutex<Option<Arc<GlobalFilePageCache>>> = Mutex::new(None);

/// 根卷重挂载或辅助卷挂载/卸载后调用：原地清空缓存并绑定新 `mount_gen`，
/// 复用已分配的帧池（不重新分配 16MiB），避免长跑大量挂载操作后堆碎片化卡死。
// 本方法代码由AI完成
pub fn reset_global_cache(mount_gen : u64) {
    let mut g = GLOBAL_CACHE.lock();
    if let Some(ref existing) = *g {
        existing.reset_to_gen(mount_gen);
    } else {
        *g = Some(Arc::new(GlobalFilePageCache::new(mount_gen)));
    }
}

/// 返回全局页缓存句柄；若代次不匹配则原地切换代次（复用帧池）。
// 本方法代码由AI完成
pub fn global_cache(mount_gen : u64) -> Arc<GlobalFilePageCache> {
    let mut g = GLOBAL_CACHE.lock();
    if let Some(ref existing) = *g {
        let current = existing.mount_gen();
        if current != mount_gen {
            if mount_gen < current {
                log::debug!("[page-cache] ignoring stale mount_gen {} (global {})",
                            mount_gen,
                            current);
                return existing.clone();
            }
            existing.reset_to_gen(mount_gen);
        }
        return existing.clone();
    }
    *g = Some(Arc::new(GlobalFilePageCache::new(mount_gen)));
    g.as_ref()
     .unwrap()
     .clone()
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::vec;
    use core::cell::Cell;

    struct CountingIo {
        reads : Cell<usize>,
        writes : usize,
        data : Vec<u8>,
    }

    struct RacingIo {
        cache : Arc<GlobalFilePageCache>,
        raced : bool,
        data : Vec<u8>,
    }

    struct FailOnceIo {
        fail_write : bool,
        data : Vec<u8>,
    }

    impl PageCacheIo for RacingIo {
        type Error = ();

        fn read_range(&self, _path : &str, _offset : u64, _buf : &mut [u8]) -> Result<usize, ()> {
            Ok(0)
        }

        fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> Result<usize, ()> {
            if !self.raced {
                self.raced = true;
                let key = self.cache.file_key(path);
                let version = {
                    let mut state = self.cache.state.lock();
                    let idx = *state.index
                                    .get(&(key.clone(), 0))
                                    .unwrap();
                    state.page_data_mut(idx).fill(0x22);
                    state.mark_dirty(idx)
                };
                let entry = self.cache.files
                                      .lock()
                                      .get(&key)
                                      .cloned()
                                      .unwrap();
                entry.write().dirty_pages.insert(0, version);
            }
            let start = offset as usize;
            let end = start + data.len();
            if self.data.len() < end {
                self.data.resize(end, 0);
            }
            self.data[start..end].copy_from_slice(data);
            Ok(data.len())
        }
    }

    impl CountingIo {
        fn new() -> Self {
            Self { reads : Cell::new(0),
                   writes : 0,
                   data : Vec::new() }
        }
    }

    impl PageCacheIo for FailOnceIo {
        type Error = ();

        fn read_range(&self, _path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, ()> {
            let start = usize::try_from(offset).map_err(|_| ())?;
            if start >= self.data.len() {
                return Ok(0);
            }
            let n = buf.len().min(self.data.len() - start);
            buf[..n].copy_from_slice(&self.data[start..start + n]);
            Ok(n)
        }

        fn write_range(&mut self, _path : &str, offset : u64, data : &[u8]) -> Result<usize, ()> {
            if self.fail_write {
                self.fail_write = false;
                return Err(());
            }
            let start = usize::try_from(offset).map_err(|_| ())?;
            let end = start.checked_add(data.len()).ok_or(())?;
            if self.data.len() < end {
                self.data.resize(end, 0);
            }
            self.data[start..end].copy_from_slice(data);
            Ok(data.len())
        }
    }

    impl PageCacheIo for CountingIo {
        type Error = ();

        fn read_range(&self, _path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, ()> {
            self.reads
                .set(self.reads.get() + 1);
            let start = usize::try_from(offset).map_err(|_| ())?;
            if start >= self.data.len() {
                return Ok(0);
            }
            let n = buf.len()
                       .min(self.data.len() - start);
            buf[..n].copy_from_slice(&self.data[start..start + n]);
            Ok(n)
        }

        fn write_range(&mut self, _path : &str, offset : u64, data : &[u8]) -> Result<usize, ()> {
            self.writes += 1;
            let start = usize::try_from(offset).map_err(|_| ())?;
            let end = start.checked_add(data.len())
                           .ok_or(())?;
            if end > self.data.len() {
                self.data
                    .resize(end, 0);
            }
            self.data[start..end].copy_from_slice(data);
            Ok(data.len())
        }
    }

    #[test]
    fn full_page_write_does_not_read_before_overwrite() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let payload = vec![0x5Au8; FILE_PAGE_SIZE];

        let n = cache.write(&mut io,
                            "/tmp/full-page",
                            0,
                            0,
                            &payload,
                            |e| e)
                     .unwrap();
        assert_eq!(n, FILE_PAGE_SIZE);
        assert_eq!(io.reads.get(), 0);

        cache.flush(&mut io, "/tmp/full-page", |e| e)
             .unwrap();
        assert_eq!(io.writes, 1);
        assert_eq!(io.data, payload);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn partial_write_to_fresh_page_does_not_read_before_overwrite() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let payload = vec![0x42u8; FILE_PAGE_SIZE / 4];

        let n = cache.write(&mut io,
                            "/tmp/partial-append",
                            0,
                            0,
                            &payload,
                            |e| e)
                     .unwrap();
        assert_eq!(n, payload.len());
        assert_eq!(io.reads.get(), 0);

        cache.flush(&mut io, "/tmp/partial-append", |e| e)
             .unwrap();
        assert_eq!(io.writes, 1);
        assert_eq!(io.data, payload);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn flush_coalesces_consecutive_dirty_pages() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let payload = vec![0x7Bu8; FILE_PAGE_SIZE * 3];

        let n = cache.write(&mut io,
                            "/tmp/three-pages",
                            0,
                            0,
                            &payload,
                            |e| e)
                     .unwrap();
        assert_eq!(n, payload.len());

        cache.flush(&mut io, "/tmp/three-pages", |e| e)
             .unwrap();
        assert_eq!(io.writes, 1);
        assert_eq!(io.data, payload);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn flush_preserves_a_write_racing_with_writeback() {
        let cache = Arc::new(GlobalFilePageCache::new(7));
        let mut io = RacingIo { cache : cache.clone(),
                                raced : false,
                                data : Vec::new() };
        let initial = vec![0x11u8; FILE_PAGE_SIZE];

        cache.write(&mut io, "/tmp/racing", 0, 0, &initial, |e| e)
             .unwrap();
        cache.flush(&mut io, "/tmp/racing", |e| e)
             .unwrap();

        assert_eq!(cache.dirty_page_count("/tmp/racing"), 1);
        assert_eq!(io.data, initial);

        cache.flush(&mut io, "/tmp/racing", |e| e)
             .unwrap();
        assert_eq!(cache.dirty_page_count("/tmp/racing"), 0);
        assert_eq!(io.data, vec![0x22u8; FILE_PAGE_SIZE]);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn failed_dirty_eviction_preserves_data_for_retry() {
        let cache = GlobalFilePageCache::new(7);
        *cache.state.lock() = GlobalCacheState::with_capacity(1);
        let payload = vec![0x5Cu8; FILE_PAGE_SIZE];
        let mut io = FailOnceIo { fail_write : true,
                                  data : Vec::new() };

        cache.write(&mut io, "/tmp/dirty", 0, 0, &payload, |e| e)
             .unwrap();
        let mut other = vec![0u8; FILE_PAGE_SIZE];
        assert!(cache.read(&mut io,
                           "/tmp/other",
                           FILE_PAGE_SIZE as u64,
                           0,
                           &mut other,
                           |e| e)
                     .is_err());

        assert_eq!(cache.dirty_page_count("/tmp/dirty"), 1);
        {
            let state = cache.state.lock();
            let idx = *state.index.get(&(cache.file_key("/tmp/dirty"), 0)).unwrap();
            assert!(state.frames[idx].dirty);
            assert_eq!(state.page_data(idx), payload.as_slice());
            state.assert_lru_invariants();
        }

        cache.read(&mut io,
                   "/tmp/other",
                   FILE_PAGE_SIZE as u64,
                   0,
                   &mut other,
                   |e| e)
             .unwrap();
        assert_eq!(io.data, payload);
        assert_eq!(cache.dirty_page_count("/tmp/dirty"), 0);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn failed_dirty_eviction_during_new_page_write_preserves_victim() {
        let cache = GlobalFilePageCache::new(7);
        *cache.state.lock() = GlobalCacheState::with_capacity(1);
        let payload = vec![0x6Du8; FILE_PAGE_SIZE];
        let replacement = vec![0x7Eu8; FILE_PAGE_SIZE];
        let mut io = FailOnceIo { fail_write : true,
                                  data : Vec::new() };

        cache.write(&mut io, "/tmp/dirty", 0, 0, &payload, |e| e)
             .unwrap();
        assert!(cache.write(&mut io,
                            "/tmp/replacement",
                            0,
                            0,
                            &replacement,
                            |e| e)
                     .is_err());
        assert_eq!(cache.dirty_page_count("/tmp/dirty"), 1);

        cache.flush(&mut io, "/tmp/dirty", |e| e).unwrap();
        assert_eq!(io.data, payload);
        assert_eq!(cache.dirty_page_count("/tmp/dirty"), 0);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn dirty_eviction_retries_when_victim_changes_during_writeback() {
        let cache = Arc::new(GlobalFilePageCache::new(7));
        *cache.state.lock() = GlobalCacheState::with_capacity(1);
        let initial = vec![0x11u8; FILE_PAGE_SIZE];
        let latest = vec![0x22u8; FILE_PAGE_SIZE];
        let mut io = RacingIo { cache : cache.clone(),
                                raced : false,
                                data : Vec::new() };

        cache.write(&mut io, "/tmp/racing", 0, 0, &initial, |e| e)
             .unwrap();
        let mut other = vec![0u8; FILE_PAGE_SIZE];
        cache.read(&mut io,
                   "/tmp/other",
                   FILE_PAGE_SIZE as u64,
                   0,
                   &mut other,
                   |e| e)
             .unwrap();

        assert!(io.raced);
        assert_eq!(io.data, latest);
        assert_eq!(cache.dirty_page_count("/tmp/racing"), 0);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn truncate_clears_cached_bytes_past_new_eof() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let payload = vec![0x66u8; FILE_PAGE_SIZE];

        cache.write(&mut io, "/tmp/truncate", 0, 0, &payload, |e| e)
             .unwrap();
        cache.flush(&mut io, "/tmp/truncate", |e| e)
             .unwrap();
        cache.truncate("/tmp/truncate", 100);
        cache.truncate("/tmp/truncate", FILE_PAGE_SIZE as u64);

        let mut out = vec![0u8; FILE_PAGE_SIZE];
        cache.read(&mut io,
                   "/tmp/truncate",
                   FILE_PAGE_SIZE as u64,
                   0,
                   &mut out,
                   |e| e)
             .unwrap();
        assert_eq!(&out[..100], &payload[..100]);
        assert!(out[100..].iter().all(|byte| *byte == 0));
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn purge_removes_cached_pages_for_recreated_path() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let old_payload = vec![0x11u8; FILE_PAGE_SIZE];

        cache.write(&mut io,
                    "/tmp/recreated",
                    0,
                    0,
                    &old_payload,
                    |e| e)
             .unwrap();
        cache.flush(&mut io, "/tmp/recreated", |e| e)
             .unwrap();
        cache.purge_closed_file("/tmp/recreated");

        io.data = vec![0x22u8; FILE_PAGE_SIZE];
        let mut out = vec![0u8; FILE_PAGE_SIZE];
        cache.read(&mut io,
                   "/tmp/recreated",
                   FILE_PAGE_SIZE as u64,
                   0,
                   &mut out,
                   |e| e)
             .unwrap();

        assert_eq!(out, io.data);
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn release_open_ref_purges_when_last_handle_closes() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let payload = vec![0x33u8; FILE_PAGE_SIZE];

        cache.acquire_open_ref("/tmp/refcount");
        cache.acquire_open_ref("/tmp/refcount");
        cache.write(&mut io,
                    "/tmp/refcount",
                    0,
                    0,
                    &payload,
                    |e| e)
             .unwrap();
        cache.flush(&mut io, "/tmp/refcount", |e| e)
             .unwrap();
        assert!(cache.files.lock().contains_key(&cache.file_key("/tmp/refcount")));

        cache.release_open_ref("/tmp/refcount");
        assert!(cache.files.lock().contains_key(&cache.file_key("/tmp/refcount")));
        cache.release_open_ref("/tmp/refcount");
        assert!(!cache.files.lock().contains_key(&cache.file_key("/tmp/refcount")));
        cache.state.lock().assert_lru_invariants();
    }

    #[test]
    fn rename_moves_source_open_refs_and_drops_target_refs() {
        let cache = GlobalFilePageCache::new(7);
        let mut io = CountingIo::new();
        let payload = vec![0x44u8; FILE_PAGE_SIZE];

        cache.acquire_open_ref("/tmp/source");
        cache.acquire_open_ref("/tmp/source");
        cache.acquire_open_ref("/tmp/target");
        cache.write(&mut io, "/tmp/source", 0, 0, &payload, |e| e)
             .unwrap();
        cache.flush(&mut io, "/tmp/source", |e| e).unwrap();

        cache.finish_rename("/tmp/source", "/tmp/target");
        assert!(!cache.files.lock().contains_key(&cache.file_key("/tmp/source")));
        assert!(!cache.files.lock().contains_key(&cache.file_key("/tmp/target")));

        cache.write(&mut io, "/tmp/target", 0, 0, &payload, |e| e)
             .unwrap();
        cache.release_open_ref("/tmp/target");
        assert!(cache.files.lock().contains_key(&cache.file_key("/tmp/target")));
        cache.release_open_ref("/tmp/target");
        assert!(!cache.files.lock().contains_key(&cache.file_key("/tmp/target")));
        cache.state.lock().assert_lru_invariants();
    }

    fn activate_test_frame(state : &mut GlobalCacheState,
                           idx : usize,
                           page_idx : u64,
                           dirty : bool) {
        if let Some(pos) = state.free.iter().position(|free_idx| *free_idx == idx) {
            state.free.swap_remove(pos);
        }
        let key = (FileCacheKey::path(7, Arc::from("/tmp/lru")), page_idx);
        state.frames[idx].key = Some(key.clone());
        state.frames[idx].dirty = false;
        state.frames[idx].version = 0;
        state.index.insert(key, idx);
        state.touch_lru(idx);
        if dirty {
            state.mark_dirty(idx);
        }
    }

    #[test]
    fn intrusive_lru_touch_and_class_transitions_preserve_invariants() {
        let mut state = GlobalCacheState::with_capacity(3);
        activate_test_frame(&mut state, 0, 0, false);
        activate_test_frame(&mut state, 1, 1, false);
        activate_test_frame(&mut state, 2, 2, false);
        state.assert_lru_invariants();
        assert_eq!(state.clean_lru_head, Some(0));
        assert_eq!(state.clean_lru_tail, Some(2));

        state.touch_lru(0);
        state.assert_lru_invariants();
        assert_eq!(state.clean_lru_head, Some(1));
        assert_eq!(state.clean_lru_tail, Some(0));

        state.mark_dirty(1);
        state.assert_lru_invariants();
        assert_eq!(state.clean_lru_head, Some(2));
        assert_eq!(state.dirty_lru_head, Some(1));

        state.mark_clean(1);
        state.assert_lru_invariants();
        assert_eq!(state.clean_lru_tail, Some(1));
        assert_eq!(state.dirty_lru_head, None);
    }

    #[test]
    fn eviction_prefers_clean_page_over_older_dirty_page() {
        let mut state = GlobalCacheState::with_capacity(2);
        activate_test_frame(&mut state, 0, 0, true);
        activate_test_frame(&mut state, 1, 1, false);
        state.assert_lru_invariants();

        assert_eq!(state.pop_free_or_lru_index(), Some(1));
        let evicted = state.detach_slot_for_reuse(1);
        assert!(evicted.is_none());
        state.return_detached_slot(1);
        state.assert_lru_invariants();
        assert_eq!(state.dirty_lru_head, Some(0));
        assert_eq!(state.clean_lru_head, None);
    }
}
