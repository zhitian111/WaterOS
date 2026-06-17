//! 全局共享文件页缓存（Direct 模式）：按 `(mount_gen, path, page_index)` 键 LRU 缓存页帧。
//!
//! 每个文件条目使用 [`Arc<RwLock<FileEntryInner>>`]：允许多个读者并发，写/刷盘独占。

#![no_std]
extern crate alloc;

use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::min;
use spin::{Mutex, RwLock};
use wateros_base_config::fs::{
    FILE_PAGE_CACHE_CAPACITY, FILE_PAGE_SIZE, FILE_READ_AHEAD_STRIDE,
};

/// 区间读写下层（通常由 `FsBridge` 委托 `ReadOnlyFs` / `ReadWriteFs`）。
pub trait PageCacheIo {
    type Error;
    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, Self::Error>;
    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<usize, Self::Error>;
}

/// 页缓存键：根卷挂载代次 + 绝对路径。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileCacheKey {
    pub mount_gen: u64,
    pub path: Arc<str>,
}

struct PageFrame {
    key: Option<(FileCacheKey, u64)>,
    data: Vec<u8>,
    dirty: bool,
}

struct GlobalCacheState {
    capacity: usize,
    frames: Vec<PageFrame>,
    index: BTreeMap<(FileCacheKey, u64), usize>,
    lru: VecDeque<usize>,
    free: Vec<usize>,
}

impl GlobalCacheState {
    fn new() -> Self {
        let cap = FILE_PAGE_CACHE_CAPACITY;
        let mut frames = Vec::new();
        let mut free = Vec::new();
        if cap > 0 {
            frames.reserve_exact(cap);
            for _ in 0..cap {
                frames.push(PageFrame {
                    key: None,
                    data: vec![0u8; FILE_PAGE_SIZE],
                    dirty: false,
                });
            }
            free.extend((0..cap).rev());
        }
        Self {
            capacity: cap,
            frames,
            index: BTreeMap::new(),
            lru: VecDeque::new(),
            free,
        }
    }

    fn touch_lru(&mut self, idx: usize) {
        if let Some(p) = self.lru.iter().position(|&x| x == idx) {
            self.lru.remove(p);
        }
        self.lru.push_back(idx);
    }

    fn pop_free_or_lru_index(&mut self) -> usize {
        self.free
            .pop()
            .unwrap_or_else(|| self.lru.pop_front().expect("lru non-empty when free empty"))
    }
}

/// 单文件逻辑大小与脏页索引（页号）。
struct FileEntryInner {
    logical_size: u64,
    dirty_pages: BTreeMap<u64, ()>,
}

/// 全局文件页缓存。
pub struct GlobalFilePageCache {
    mount_gen: u64,
    state: Mutex<GlobalCacheState>,
    files: Mutex<BTreeMap<FileCacheKey, Arc<RwLock<FileEntryInner>>>>,
}

impl GlobalFilePageCache {
    /// 构造与当前 `mount_gen` 绑定的缓存表。
    pub fn new(mount_gen: u64) -> Self {
        Self {
            mount_gen,
            state: Mutex::new(GlobalCacheState::new()),
            files: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn mount_gen(&self) -> u64 {
        self.mount_gen
    }

    fn file_key(&self, path: &str) -> FileCacheKey {
        FileCacheKey {
            mount_gen: self.mount_gen,
            path: Arc::from(path),
        }
    }

    fn get_file_entry(&self, path: &str, initial_size: u64) -> Arc<RwLock<FileEntryInner>> {
        let key = self.file_key(path);
        let mut files = self.files.lock();
        if let Some(e) = files.get(&key) {
            return e.clone();
        }
        let e = Arc::new(RwLock::new(FileEntryInner {
            logical_size: initial_size,
            dirty_pages: BTreeMap::new(),
        }));
        files.insert(key, e.clone());
        e
    }

    fn flush_frame<Io, E>(
        &self,
        io: &mut Io,
        slot: usize,
        logical_size_hint: Option<u64>,
        map_err: fn(Io::Error) -> E,
    ) -> Result<(), E>
    where
        Io: PageCacheIo,
    {
        let mut cache = self.state.lock();
        let Some((ref key, page_idx)) = cache.frames[slot].key.clone() else {
            return Ok(());
        };
        if !cache.frames[slot].dirty {
            return Ok(());
        }
        let off = page_idx * FILE_PAGE_SIZE as u64;
        let mut len = FILE_PAGE_SIZE;
        if let Some(size) = logical_size_hint {
            if off >= size {
                cache.frames[slot].dirty = false;
                return Ok(());
            }
            len = len.min(usize::try_from(size - off).unwrap_or(0));
        }
        let data = cache.frames[slot].data[..len].to_vec();
        cache.frames[slot].dirty = false;
        drop(cache);
        io.write_range(key.path.as_ref(), off, &data).map_err(map_err)?;
        Ok(())
    }

    fn install_page<Io, E>(
        &self,
        io: &mut Io,
        path: &str,
        page_idx: u64,
        file_size: u64,
        map_err: fn(Io::Error) -> E,
    ) -> Result<(), E>
    where
        Io: PageCacheIo,
    {
        let key = self.file_key(path);
        {
            let mut cache = self.state.lock();
            if cache.capacity == 0 {
                return Ok(());
            }
            if let Some(&idx) = cache.index.get(&(key.clone(), page_idx)) {
                cache.touch_lru(idx);
                return Ok(());
            }
        }

        let page_off = page_idx * FILE_PAGE_SIZE as u64;
        let mut page_buf = vec![0u8; FILE_PAGE_SIZE];
        if page_off < file_size {
            let to_read = FILE_PAGE_SIZE.min(
                usize::try_from(file_size.saturating_sub(page_off)).unwrap_or(0),
            );
            if to_read > 0 {
                let n = io
                    .read_range(path, page_off, &mut page_buf[..to_read])
                    .map_err(map_err)?;
                if n < to_read {
                    page_buf[n..to_read].fill(0);
                }
            }
        }

        let mut cache = self.state.lock();
        if cache.capacity == 0 {
            return Ok(());
        }
        if let Some(&idx) = cache.index.get(&(key.clone(), page_idx)) {
            cache.touch_lru(idx);
            return Ok(());
        }

        let idx = cache.pop_free_or_lru_index();
        if cache.frames[idx].dirty {
            drop(cache);
            self.flush_frame(io, idx, None, map_err)?;
            cache = self.state.lock();
        }
        if let Some(old) = cache.frames[idx].key.take() {
            cache.index.remove(&old);
            if let Some(p) = cache.lru.iter().position(|&x| x == idx) {
                cache.lru.remove(p);
            }
        }
        cache.frames[idx].data.copy_from_slice(&page_buf);
        cache.frames[idx].dirty = false;
        cache.frames[idx].key = Some((key.clone(), page_idx));
        cache.index.insert((key, page_idx), idx);
        cache.touch_lru(idx);
        Ok(())
    }

    /// Direct 模式：从 `offset` 读入 `buf`。
    pub fn read<Io, E>(
        &self,
        io: &mut Io,
        path: &str,
        file_size: u64,
        offset: u64,
        buf: &mut [u8],
        map_err: fn(Io::Error) -> E,
    ) -> Result<usize, E>
    where
        Io: PageCacheIo,
    {
        if buf.is_empty() || offset >= file_size {
            return Ok(0);
        }
        let entry = self.get_file_entry(path, file_size);
        let _guard = entry.read();
        let max = min(
            buf.len(),
            usize::try_from(file_size - offset).unwrap_or(0),
        );
        let mut done = 0usize;
        let mut pos = offset;
        while done < max {
            let page_idx = pos / FILE_PAGE_SIZE as u64;
            let page_off = (pos % FILE_PAGE_SIZE as u64) as usize;
            let chunk = (FILE_PAGE_SIZE - page_off).min(max - done);
            self.install_page(io, path, page_idx, file_size, map_err)?;
            let cache = self.state.lock();
            let idx = *cache
                .index
                .get(&(self.file_key(path), page_idx))
                .expect("page installed");
            buf[done..done + chunk]
                .copy_from_slice(&cache.frames[idx].data[page_off..page_off + chunk]);
            drop(cache);
            done += chunk;
            pos += chunk as u64;
        }
        if FILE_READ_AHEAD_STRIDE > 0 {
            let start_page = offset / FILE_PAGE_SIZE as u64 + 1;
            for ahead in 0..FILE_READ_AHEAD_STRIDE {
                let pi = start_page + ahead as u64;
                if pi * FILE_PAGE_SIZE as u64 >= file_size {
                    break;
                }
                let _ = self.install_page(io, path, pi, file_size, map_err);
            }
        }
        Ok(done)
    }

    /// Direct 模式：从 `offset` 写入 `buf`。
    pub fn write<Io, E>(
        &self,
        io: &mut Io,
        path: &str,
        file_size: u64,
        offset: u64,
        buf: &[u8],
        map_err: fn(Io::Error) -> E,
    ) -> Result<usize, E>
    where
        Io: PageCacheIo,
    {
        if buf.is_empty() {
            return Ok(0);
        }
        let entry = self.get_file_entry(path, file_size);
        let mut guard = entry.write();
        let mut pos = offset;
        let mut written = 0usize;
        while written < buf.len() {
            let page_idx = pos / FILE_PAGE_SIZE as u64;
            let page_off = (pos % FILE_PAGE_SIZE as u64) as usize;
            let chunk = (FILE_PAGE_SIZE - page_off).min(buf.len() - written);
            let hint_size = guard.logical_size.max(pos + chunk as u64);
            self.install_page(io, path, page_idx, hint_size, map_err)?;
            {
                let mut cache = self.state.lock();
                let idx = *cache
                    .index
                    .get(&(self.file_key(path), page_idx))
                    .expect("page for write");
                cache.frames[idx].data[page_off..page_off + chunk]
                    .copy_from_slice(&buf[written..written + chunk]);
                cache.frames[idx].dirty = true;
                cache.touch_lru(idx);
            }
            guard.dirty_pages.insert(page_idx, ());
            let end = pos + chunk as u64;
            if end > guard.logical_size {
                guard.logical_size = end;
            }
            written += chunk;
            pos += chunk as u64;
        }
        Ok(written)
    }

    pub fn logical_size(&self, path: &str, fallback: u64) -> u64 {
        let key = self.file_key(path);
        let files = self.files.lock();
        files
            .get(&key)
            .map(|e| e.read().logical_size)
            .unwrap_or(fallback)
    }

    pub fn set_logical_size(&self, path: &str, size: u64) {
        let entry = self.get_file_entry(path, size);
        entry.write().logical_size = size;
    }

    /// 将脏页写回下层并清除脏标记。
    pub fn flush<Io, E>(
        &self,
        io: &mut Io,
        path: &str,
        map_err: fn(Io::Error) -> E,
    ) -> Result<(), E>
    where
        Io: PageCacheIo,
    {
        let key = self.file_key(path);
        let entry = {
            let files = self.files.lock();
            files.get(&key).cloned()
        };
        let Some(entry) = entry else {
            return Ok(());
        };
        let mut guard = entry.write();
        let dirty: Vec<u64> = guard.dirty_pages.keys().copied().collect();
        for page_idx in dirty {
            let slot = {
                let cache = self.state.lock();
                cache.index.get(&(key.clone(), page_idx)).copied()
            };
            let Some(slot) = slot else {
                guard.dirty_pages.remove(&page_idx);
                continue;
            };
            self.flush_frame(io, slot, Some(guard.logical_size), map_err)?;
            guard.dirty_pages.remove(&page_idx);
        }
        Ok(())
    }

    /// 更新逻辑长度，并丢弃 EOF 之后的缓存页。
    pub fn truncate(&self, path: &str, len: u64) {
        let key = self.file_key(path);
        let entry = self.get_file_entry(path, len);
        {
            let mut guard = entry.write();
            guard.logical_size = len;
            let first_past_eof = if len == 0 {
                0
            } else {
                (len - 1) / FILE_PAGE_SIZE as u64 + 1
            };
            let to_remove: Vec<u64> = guard
                .dirty_pages
                .keys()
                .copied()
                .filter(|page_idx| *page_idx >= first_past_eof)
                .collect();
            for page_idx in to_remove {
                guard.dirty_pages.remove(&page_idx);
            }
        }

        let mut cache = self.state.lock();
        let first_past_eof = if len == 0 {
            0
        } else {
            (len - 1) / FILE_PAGE_SIZE as u64 + 1
        };
        let keys_to_remove: Vec<(FileCacheKey, u64)> = cache
            .index
            .keys()
            .filter(|(k, page_idx)| *k == key && *page_idx >= first_past_eof)
            .cloned()
            .collect();
        for old in keys_to_remove {
            if let Some(slot) = cache.index.remove(&old) {
                cache.frames[slot].key = None;
                cache.frames[slot].dirty = false;
                if let Some(p) = cache.lru.iter().position(|&x| x == slot) {
                    cache.lru.remove(p);
                }
                cache.free.push(slot);
            }
        }

        if len > 0 {
            let tail = (len % FILE_PAGE_SIZE as u64) as usize;
            if tail > 0 {
                let page_idx = len / FILE_PAGE_SIZE as u64;
                if let Some(&slot) = cache.index.get(&(key, page_idx)) {
                    cache.frames[slot].data[tail..].fill(0);
                }
            }
        }
    }
}

static GLOBAL_CACHE: Mutex<Option<Arc<GlobalFilePageCache>>> = Mutex::new(None);

/// 根卷重挂载后调用：丢弃旧代次缓存并绑定新 `mount_gen`。
pub fn reset_global_cache(mount_gen: u64) {
    *GLOBAL_CACHE.lock() = Some(Arc::new(GlobalFilePageCache::new(mount_gen)));
}

/// 返回全局页缓存句柄；若代次不匹配则重建。
pub fn global_cache(mount_gen: u64) -> Arc<GlobalFilePageCache> {
    let mut g = GLOBAL_CACHE.lock();
    let rebuild = g
        .as_ref()
        .map(|c| c.mount_gen() != mount_gen)
        .unwrap_or(true);
    if rebuild {
        *g = Some(Arc::new(GlobalFilePageCache::new(mount_gen)));
    }
    g.as_ref().unwrap().clone()
}
