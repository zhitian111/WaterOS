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
use wateros_base_config::fs::{FILE_PAGE_CACHE_CAPACITY, FILE_PAGE_SIZE, FILE_READ_AHEAD_STRIDE};

const FLUSH_RUN_MAX_PAGES : usize = 64;

/// 区间读写下层（通常由 `FsBridge` 委托 `ReadOnlyFs` / `ReadWriteFs`）。
pub trait PageCacheIo {
    type Error;
    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, Self::Error>;
    fn write_range(&mut self,
                   path : &str,
                   offset : u64,
                   data : &[u8])
                   -> Result<usize, Self::Error>;
}

/// 页缓存键：根卷挂载代次 + 绝对路径。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileCacheKey {
    pub mount_gen : u64,
    pub path : Arc<str>,
}

struct PageFrame {
    key : Option<(FileCacheKey, u64)>,
    data : Vec<u8>,
    dirty : bool,
}

struct GlobalCacheState {
    capacity : usize,
    frames : Vec<PageFrame>,
    index : BTreeMap<(FileCacheKey, u64), usize>,
    lru : VecDeque<usize>,
    free : Vec<usize>,
}

impl GlobalCacheState {
    fn new() -> Self {
        let cap = FILE_PAGE_CACHE_CAPACITY;
        let mut frames = Vec::new();
        let mut free = Vec::new();
        if cap > 0 {
            frames.reserve_exact(cap);
            for _ in 0..cap {
                frames.push(PageFrame { key : None,
                                        data : vec![0u8; FILE_PAGE_SIZE],
                                        dirty : false });
            }
            free.extend((0..cap).rev());
        }
        Self { capacity : cap,
               frames,
               index : BTreeMap::new(),
               lru : VecDeque::new(),
               free }
    }

    fn touch_lru(&mut self, idx : usize) {
        if let Some(p) = self.lru
                             .iter()
                             .position(|&x| x == idx)
        {
            self.lru.remove(p);
        }
        self.lru
            .push_back(idx);
    }

    fn pop_free_or_lru_index(&mut self) -> usize {
        if let Some(idx) = self.free.pop() {
            return idx;
        }
        if let Some(idx) = self.lru.pop_front() {
            return idx;
        }
        // 兜底：从 index 中驱逐第一个条目，防止 free 和 lru 双空时 panic
        let victim_key = self.index
                             .keys()
                             .next()
                             .cloned()
                             .expect("page cache: no entries to evict");
        let idx = self.index
                      .remove(&victim_key)
                      .unwrap();
        self.frames[idx].key = None;
        self.frames[idx].dirty = false;
        idx
    }

    fn detach_slot_for_reuse(&mut self,
                             idx : usize)
                             -> Option<((FileCacheKey, u64), Vec<u8>)> {
        let old = self.frames[idx].key.take();
        if let Some(ref key) = old {
            self.index.remove(key);
        }
        if let Some(p) = self.lru
                          .iter()
                          .position(|&x| x == idx)
        {
            self.lru.remove(p);
        }
        let dirty_data = if self.frames[idx].dirty {
            old.clone()
               .map(|key| (key, self.frames[idx].data.clone()))
        } else {
            None
        };
        self.frames[idx].dirty = false;
        dirty_data
    }

    fn return_detached_slot(&mut self, idx : usize) {
        if self.frames[idx].key.is_none() &&
           !self.free
                .iter()
                .any(|&free_idx| free_idx == idx)
        {
            self.free.push(idx);
        }
    }
}

/// 单文件逻辑大小与脏页索引（页号）。
struct FileEntryInner {
    logical_size : u64,
    dirty_pages : BTreeMap<u64, ()>,
}

/// 全局文件页缓存。
pub struct GlobalFilePageCache {
    mount_gen : u64,
    state : Mutex<GlobalCacheState>,
    files : Mutex<BTreeMap<FileCacheKey, Arc<RwLock<FileEntryInner>>>>,
}

impl GlobalFilePageCache {
    /// 构造与当前 `mount_gen` 绑定的缓存表。
    pub fn new(mount_gen : u64) -> Self {
        Self { mount_gen,
               state : Mutex::new(GlobalCacheState::new()),
               files : Mutex::new(BTreeMap::new()) }
    }

    pub fn mount_gen(&self) -> u64 { self.mount_gen }

    fn file_key(&self, path : &str) -> FileCacheKey {
        FileCacheKey { mount_gen : self.mount_gen,
                       path : Arc::from(path) }
    }

    fn get_file_entry(&self, path : &str, initial_size : u64) -> Arc<RwLock<FileEntryInner>> {
        let key = self.file_key(path);
        let mut files = self.files.lock();
        if let Some(e) = files.get(&key) {
            return e.clone();
        }
        let e = Arc::new(RwLock::new(FileEntryInner { logical_size : initial_size,
                                                      dirty_pages : BTreeMap::new() }));
        files.insert(key, e.clone());
        e
    }


    fn queue_flush_batch(batches : &mut Vec<(u64, Vec<u8>)>,
                         batch_start : &mut Option<u64>,
                         batch_data : &mut Vec<u8>) {
        if let Some(start_page) = batch_start.take() {
            if !batch_data.is_empty() {
                batches.push((start_page, core::mem::take(batch_data)));
            }
        }
    }

    fn flush_dirty_run<Io, E>(&self,
                              io : &mut Io,
                              key : &FileCacheKey,
                              pages : &[u64],
                              logical_size : u64,
                              map_err : fn(Io::Error) -> E)
                              -> Result<Vec<u64>, E>
        where Io : PageCacheIo
    {
        let mut batches : Vec<(u64, Vec<u8>)> = Vec::new();
        let mut batch_start = None;
        let mut batch_last = None;
        let mut batch_data = Vec::with_capacity(pages.len()
                                                     .min(FLUSH_RUN_MAX_PAGES) *
                                                FILE_PAGE_SIZE);
        let mut flushed_pages = Vec::new();

        {
            let mut cache = self.state.lock();
            for &page_idx in pages {
                let off = page_idx * FILE_PAGE_SIZE as u64;
                let slot = cache.index
                                .get(&(key.clone(), page_idx))
                                .copied();
                let Some(slot) = slot else {
                    Self::queue_flush_batch(&mut batches,
                                            &mut batch_start,
                                            &mut batch_data);
                    batch_last = None;
                    flushed_pages.push(page_idx);
                    continue;
                };
                if off >= logical_size || !cache.frames[slot].dirty {
                    cache.frames[slot].dirty = false;
                    Self::queue_flush_batch(&mut batches,
                                            &mut batch_start,
                                            &mut batch_data);
                    batch_last = None;
                    flushed_pages.push(page_idx);
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
                batch_data.extend_from_slice(&cache.frames[slot].data[..len]);
                cache.frames[slot].dirty = false;
                batch_last = Some(page_idx);
                flushed_pages.push(page_idx);
            }
            Self::queue_flush_batch(&mut batches,
                                    &mut batch_start,
                                    &mut batch_data);
        }

        for (start_page, data) in batches {
            let off = start_page * FILE_PAGE_SIZE as u64;
            log::info!("[iozone-probe][page-cache-run] begin path={} start_page={} offset={} len={}",
                       key.path.as_ref(),
                       start_page,
                       off,
                       data.len());
            io.write_range(key.path.as_ref(), off, &data)
              .map_err(map_err)?;
            log::info!("[iozone-probe][page-cache-run] end path={} start_page={} offset={} len={}",
                       key.path.as_ref(),
                       start_page,
                       off,
                       data.len());
        }

        Ok(flushed_pages)
    }

    fn install_page<Io, E>(&self,
                           io : &mut Io,
                           path : &str,
                           page_idx : u64,
                           file_size : u64,
                           map_err : fn(Io::Error) -> E)
                           -> Result<(), E>
        where Io : PageCacheIo
    {
        let key = self.file_key(path);
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
        let mut page_buf = vec![0u8; FILE_PAGE_SIZE];
        if page_off < file_size {
            let to_read = FILE_PAGE_SIZE.min(
                usize::try_from(file_size.saturating_sub(page_off)).unwrap_or(0),
            );
            if to_read > 0 {
                let n = io.read_range(path, page_off, &mut page_buf[..to_read])
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

        let idx = cache.pop_free_or_lru_index();
        let pending_flush = cache.detach_slot_for_reuse(idx);
        if let Some(((ref k, page_idx), ref saved_data)) = pending_flush {
            drop(cache);
            {
                let off = page_idx * FILE_PAGE_SIZE as u64;
                if let Err(err) = io.write_range(k.path.as_ref(),
                                                 off,
                                                 &saved_data[..FILE_PAGE_SIZE])
                {
                    let mut cache = self.state.lock();
                    cache.return_detached_slot(idx);
                    return Err(map_err(err));
                }
            }
            cache = self.state.lock();
        }

        cache.frames[idx].data
                         .copy_from_slice(&page_buf);
        cache.frames[idx].dirty = false;
        cache.frames[idx].key = Some((key.clone(), page_idx));
        cache.index
             .insert((key, page_idx), idx);
        cache.touch_lru(idx);
        Ok(())
    }

    fn install_zero_page<Io, E>(&self,
                                io : &mut Io,
                                path : &str,
                                page_idx : u64,
                                map_err : fn(Io::Error) -> E)
                                -> Result<(), E>
        where Io : PageCacheIo
    {
        let key = self.file_key(path);
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

        let idx = cache.pop_free_or_lru_index();
        let pending_flush = cache.detach_slot_for_reuse(idx);
        if let Some(((ref k, page_idx), ref saved_data)) = pending_flush {
            drop(cache);
            {
                let off = page_idx * FILE_PAGE_SIZE as u64;
                if let Err(err) = io.write_range(k.path.as_ref(),
                                                 off,
                                                 &saved_data[..FILE_PAGE_SIZE])
                {
                    let mut cache = self.state.lock();
                    cache.return_detached_slot(idx);
                    return Err(map_err(err));
                }
            }
            cache = self.state.lock();
        }
        cache.frames[idx].data
                         .fill(0);
        cache.frames[idx].dirty = false;
        cache.frames[idx].key = Some((key.clone(), page_idx));
        cache.index
             .insert((key, page_idx), idx);
        cache.touch_lru(idx);
        Ok(())
    }

    /// Direct 模式：从 `offset` 读入 `buf`。
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
        if buf.is_empty() || offset >= file_size {
            return Ok(0);
        }
        let entry = self.get_file_entry(path, file_size);
        let _guard = entry.read();
        let max = min(buf.len(),
                      usize::try_from(file_size - offset).unwrap_or(0));
        let mut done = 0usize;
        let mut pos = offset;
        while done < max {
            let page_idx = pos / FILE_PAGE_SIZE as u64;
            let page_off = (pos % FILE_PAGE_SIZE as u64) as usize;
            let chunk = (FILE_PAGE_SIZE - page_off).min(max - done);
            self.install_page(io, path, page_idx, file_size, map_err)?;
            let cache = self.state.lock();
            let idx = *cache.index
                            .get(&(self.file_key(path), page_idx))
                            .expect("page installed");
            buf[done..done + chunk].copy_from_slice(&cache.frames[idx].data
                                                        [page_off..page_off + chunk]);
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
            let page_start = page_idx * FILE_PAGE_SIZE as u64;
            if page_start >= guard.logical_size || (page_off == 0 && chunk == FILE_PAGE_SIZE) {
                self.install_zero_page(io, path, page_idx, map_err)?;
            } else {
                self.install_page(io,
                                  path,
                                  page_idx,
                                  guard.logical_size,
                                  map_err)?;
            }
            {
                let mut cache = self.state.lock();
                let idx = *cache.index
                                .get(&(self.file_key(path), page_idx))
                                .expect("page for write");
                cache.frames[idx].data[page_off..page_off + chunk].copy_from_slice(&buf[written..
                                                                                        written +
                                                                                        chunk]);
                cache.frames[idx].dirty = true;
                cache.touch_lru(idx);
            }
            guard.dirty_pages
                 .insert(page_idx, ());
            let end = pos + chunk as u64;
            if end > guard.logical_size {
                guard.logical_size = end;
            }
            written += chunk;
            pos += chunk as u64;
        }
        Ok(written)
    }

    pub fn logical_size(&self, path : &str, fallback : u64) -> u64 {
        let key = self.file_key(path);
        let files = self.files.lock();
        files.get(&key)
             .map(|e| {
                 e.read()
                  .logical_size
             })
             .unwrap_or(fallback)
    }

    pub fn set_logical_size(&self, path : &str, size : u64) {
        let entry = self.get_file_entry(path, size);
        entry.write()
             .logical_size = size;
    }

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
    pub fn flush<Io, E>(&self,
                        io : &mut Io,
                        path : &str,
                        map_err : fn(Io::Error) -> E)
                        -> Result<(), E>
        where Io : PageCacheIo
    {
        let key = self.file_key(path);
        let entry = {
            let files = self.files.lock();
            files.get(&key)
                 .cloned()
        };
        let Some(entry) = entry else {
            log::info!("[iozone-probe][page-cache-flush] path={} no-entry", path);
            return Ok(());
        };
        let mut guard = entry.write();
        let dirty : Vec<u64> = guard.dirty_pages
                                    .keys()
                                    .copied()
                                    .collect();
        log::info!("[iozone-probe][page-cache-flush] begin path={} logical_size={} dirty_pages={}",
                   path,
                   guard.logical_size,
                   dirty.len());

        let mut run = Vec::new();
        for page_idx in dirty {
            let should_flush = run.last()
                                  .is_some_and(|last| *last + 1 != page_idx) ||
                               run.len() >= FLUSH_RUN_MAX_PAGES;
            if should_flush {
                let flushed = self.flush_dirty_run(io,
                                                   &key,
                                                   &run,
                                                   guard.logical_size,
                                                   map_err)?;
                for flushed_page in flushed {
                    guard.dirty_pages
                         .remove(&flushed_page);
                }
                run.clear();
            }
            run.push(page_idx);
        }
        if !run.is_empty() {
            let flushed = self.flush_dirty_run(io,
                                               &key,
                                               &run,
                                               guard.logical_size,
                                               map_err)?;
            for flushed_page in flushed {
                guard.dirty_pages
                     .remove(&flushed_page);
            }
        }
        log::info!("[iozone-probe][page-cache-flush] end path={} dirty_pages_after={}",
                   path,
                   guard.dirty_pages.len());
        Ok(())
    }

    /// 删除已关闭文件的缓存条目，释放 `dirty_pages`、`FileEntryInner` 和路径字符串的内存。
    /// 应在 VFS `close` 或 `unlink` 之后调用，防止 `files` BTreeMap 无限增长耗尽内核堆。
    pub fn purge_closed_file(&self, path : &str) {
        let key = self.file_key(path);
        let mut files = self.files.lock();
        files.remove(&key);
    }

    /// 更新逻辑长度，并丢弃 EOF 之后的缓存页。
    pub fn truncate(&self, path : &str, len : u64) {
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
                 .keys()
                 .filter(|(k, page_idx)| *k == key && *page_idx >= first_past_eof)
                 .cloned()
                 .collect();
        for old in keys_to_remove {
            if let Some(slot) = cache.index
                                     .remove(&old)
            {
                cache.frames[slot].key = None;
                cache.frames[slot].dirty = false;
                if let Some(p) = cache.lru
                                      .iter()
                                      .position(|&x| x == slot)
                {
                    cache.lru.remove(p);
                }
                cache.free
                     .push(slot);
            }
        }

        if len > 0 {
            let tail = (len % FILE_PAGE_SIZE as u64) as usize;
            if tail > 0 {
                let page_idx = len / FILE_PAGE_SIZE as u64;
                if let Some(&slot) = cache.index
                                          .get(&(key, page_idx))
                {
                    cache.frames[slot].data[tail..].fill(0);
                }
            }
        }
    }
}

static GLOBAL_CACHE : Mutex<Option<Arc<GlobalFilePageCache>>> = Mutex::new(None);

/// 根卷重挂载后调用：丢弃旧代次缓存并绑定新 `mount_gen`。
pub fn reset_global_cache(mount_gen : u64) {
    *GLOBAL_CACHE.lock() = Some(Arc::new(GlobalFilePageCache::new(mount_gen)));
}

/// 返回全局页缓存句柄；若代次不匹配则重建。
pub fn global_cache(mount_gen : u64) -> Arc<GlobalFilePageCache> {
    let mut g = GLOBAL_CACHE.lock();
    let rebuild = g.as_ref()
                   .map(|c| c.mount_gen() != mount_gen)
                   .unwrap_or(true);
    if rebuild {
        *g = Some(Arc::new(GlobalFilePageCache::new(mount_gen)));
    }
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

    impl CountingIo {
        fn new() -> Self {
            Self { reads : Cell::new(0),
                   writes : 0,
                   data : Vec::new() }
        }
    }

    impl PageCacheIo for CountingIo {
        type Error = ();

        fn read_range(&self, _path : &str, _offset : u64, buf : &mut [u8]) -> Result<usize, ()> {
            self.reads
                .set(self.reads.get() + 1);
            buf.fill(0xCC);
            Ok(buf.len())
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
    }
}
