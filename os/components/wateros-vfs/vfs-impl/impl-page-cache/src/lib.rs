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
/// 单次刷盘批次允许合并的连续页上限，限制临时缓冲区占用与单次 I/O 时延。
const FLUSH_RUN_MAX_PAGES : usize = 64;
#[cfg(feature = "diagnostics")]
const DIAGNOSTIC_REPORT_LOOKUPS : u64 = 1 << 18;

#[derive(Clone, Copy, PartialEq, Eq)]
enum InstallSource {
    /// 由调用者实际读取或写入触发，必须优先完成。
    Demand,
    /// 顺序读取预测得到；失败或被淘汰不应改变用户可见语义。
    Prefetch,
}

#[cfg(feature = "diagnostics")]
#[derive(Default)]
struct PageCacheDiagnostics {
    demand_lookups : u64,
    prefetch_lookups : u64,
    hits : u64,
    misses : u64,
    installs : u64,
    duplicate_loads : u64,
    clean_evictions : u64,
    dirty_evictions : u64,
    unused_evictions : u64,
    prefetch_installs : u64,
    prefetch_uses : u64,
    next_report : u64,
}

/// 区间读写下层（通常由 `FsBridge` 委托 `ReadOnlyFs` / `ReadWriteFs`）。
pub trait PageCacheIo {
    /// 下层 I/O 返回的具体错误类型，由 VFS 边界转换为 errno。
    type Error;
// 本方法代码由AI完成
    /// 从 `path` 的字节偏移 `offset` 读取至 `buf`，返回实际字节数；短读表示 EOF 或后端短读。
    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, Self::Error>;
// 本方法代码由AI完成
    /// 将 `data` 写到 `path` 的字节偏移 `offset`，返回实际写入字节数或后端错误。
    fn write_range(&mut self,
                   path : &str,
                   offset : u64,
                   data : &[u8])
                   -> Result<usize, Self::Error>;
}

/// 页缓存键：根卷挂载代次 + 绝对路径；可带稳定文件 node id 加速 BTree 比较。
#[derive(Clone, Debug)]
// 本结构代码由AI完成
/// 唯一标识缓存页所属的挂载实例与文件。
pub struct FileCacheKey {
    /// 根卷装载代次；代次变化后旧缓存不得用于新文件系统实例。
    pub mount_gen : u64,
    /// 稳定文件的 `(mount_id, node_id)`；`None` 表示没有稳定 node 的路径键。
    pub stable : Option<(u64, u64)>,
    /// 不能取得稳定 node id 时使用的规范化绝对路径。
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

#[path = "cache_state.rs"]
mod cache_state;
use cache_state::GlobalCacheState;
#[path = "file_cache.rs"]
mod file_cache;
pub use file_cache::GlobalFilePageCache;
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
                // 延迟到达的旧挂载请求不能回退全局代次，否则会让新根卷读取旧缓存页。
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

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[vfs/page-cache] self_test begin");
    let cache = GlobalFilePageCache::new(7);
    assert_eq!(cache.mount_gen(), 7);
    cache.reset_to_gen(8);
    assert_eq!(cache.mount_gen(), 8);
    log::info!("[vfs/page-cache] self_test complete");
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
        let evicted = state.detach_slot_for_reuse(1, false);
        assert!(evicted.is_none());
        state.return_detached_slot(1);
        state.assert_lru_invariants();
        assert_eq!(state.dirty_lru_head, Some(0));
        assert_eq!(state.clean_lru_head, None);
    }
}
