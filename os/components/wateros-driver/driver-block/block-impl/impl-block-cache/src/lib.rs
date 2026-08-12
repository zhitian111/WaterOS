//! 块设备写穿（write-through）LRU 缓存：包装任意 [`BlockDevice`]，对上仍实现同一 trait。
//!
//! 连续未命中区间合并为单次底层 [`BlockDevice::read_blocks`]，减少 VirtIO 等后端往返；
//! 读数据采用二次命中准入，避免顺序扫描把一次性块复制进数据缓存。

#![no_std]
extern crate alloc;

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use spin::Mutex;
use wateros_base_config::fs::BLOCK_CACHE_CAPACITY_BLOCKS;

mod manager;
pub use manager::BlockCacheManager;

/// 缓存容量与策略参数（v1 仅容量）。
#[derive(Debug, Clone, Copy)]
pub struct BlockCacheConfig {
    /// 可缓存的逻辑块数量；为 `0` 时退化为直接透传底层设备（不分配槽位）。
    pub capacity_blocks: usize,
}

impl Default for BlockCacheConfig {
    fn default() -> Self {
        Self {
            capacity_blocks: BLOCK_CACHE_CAPACITY_BLOCKS,
        }
    }
}

#[derive(Clone, Copy)]
struct LbaIndexEntry {
    lba : Lba,
    idx : usize,
}

struct LbaIndex {
    buckets : Vec<[Option<LbaIndexEntry>; LBA_INDEX_WAYS]>,
    next_victim : Vec<u8>,
}

const LBA_INDEX_WAYS : usize = 8;

const RECENT_INDEX_WAYS : usize = 4;
#[cfg(feature = "diagnostics")]
const DIAGNOSTIC_REPORT_BLOCKS : u64 = 1 << 20;

/// Approximate recent-miss/refault history used for two-hit read admission.
///
/// The table deliberately stores no data and tolerates replacement: a false
/// negative only delays admission until another read, while false positives
/// are impossible because the complete LBA is compared.
struct RecentIndex {
    buckets : Vec<[Option<Lba>; RECENT_INDEX_WAYS]>,
    next : Vec<u8>,
}

impl RecentIndex {
    fn new(capacity : usize) -> Self {
        // Keep at most 50% occupancy when the history contains twice as many
        // entries as data slots. This avoids recreating the old full-table
        // conflict problem in the admission index.
        let bucket_count = capacity.div_ceil(RECENT_INDEX_WAYS / 2)
                                   .max(1);
        Self { buckets : vec![[None; RECENT_INDEX_WAYS]; bucket_count],
               next : vec![0; bucket_count] }
    }

    fn bucket(&self, lba : Lba) -> usize { (lba.0 as usize) % self.buckets.len() }

    fn take(&mut self, lba : Lba) -> bool {
        let bucket = self.bucket(lba);
        for entry in &mut self.buckets[bucket] {
            if *entry == Some(lba) {
                *entry = None;
                return true;
            }
        }
        false
    }

    fn insert(&mut self, lba : Lba) {
        let bucket = self.bucket(lba);
        if self.buckets[bucket].iter()
                               .any(|entry| *entry == Some(lba))
        {
            return;
        }
        if let Some(entry) = self.buckets[bucket].iter_mut()
                                                 .find(|entry| entry.is_none())
        {
            *entry = Some(lba);
            return;
        }
        let way = self.next[bucket] as usize;
        self.buckets[bucket][way] = Some(lba);
        self.next[bucket] = ((way + 1) % RECENT_INDEX_WAYS) as u8;
    }
}

#[cfg(feature = "diagnostics")]
#[derive(Default)]
struct BlockCacheDiagnostics {
    read_blocks : u64,
    hit_blocks : u64,
    miss_blocks : u64,
    backend_read_calls : u64,
    backend_read_blocks : u64,
    write_blocks : u64,
    write_allocations : u64,
    capacity_evictions : u64,
    index_conflict_evictions : u64,
    ghost_hits : u64,
    next_report : u64,
}

impl LbaIndex {
    fn new(capacity : usize) -> Self {
        // Keep index occupancy at or below 50% when all data slots are live.
        // The old 100%-full table turned ordinary hash imbalance into millions
        // of conflict evictions even though the data cache itself had room.
        let bucket_count = capacity
                               .div_ceil(LBA_INDEX_WAYS / 2)
                               .max(1);
        Self { buckets : vec![[None; LBA_INDEX_WAYS]; bucket_count],
               next_victim : vec![0; bucket_count] }
    }

    fn bucket(&self, lba : Lba) -> usize {
        (lba.0 as usize) % self.buckets.len()
    }

    fn get(&self, lba : Lba) -> Option<usize> {
        let bucket = self.bucket(lba);
        self.buckets[bucket]
            .iter()
            .find_map(|entry| {
                entry.and_then(|entry| {
                    (entry.lba == lba).then_some(entry.idx)
                })
            })
    }

    fn insert(&mut self, lba : Lba, idx : usize) -> Option<(Lba, usize)> {
        let bucket = self.bucket(lba);
        let entries = &mut self.buckets[bucket];
        for entry in entries.iter_mut() {
            if let Some(entry) = entry {
                if entry.lba == lba {
                    entry.idx = idx;
                    return None;
                }
            } else {
                *entry = Some(LbaIndexEntry { lba, idx });
                return None;
            }
        }
        let victim = self.next_victim[bucket] as usize;
        self.next_victim[bucket] = ((victim + 1) % LBA_INDEX_WAYS) as u8;
        let old = entries[victim].take();
        entries[victim] = Some(LbaIndexEntry { lba, idx });
        old.map(|entry| (entry.lba, entry.idx))
    }

    fn remove(&mut self, lba : Lba) -> Option<usize> {
        let bucket = self.bucket(lba);
        let entries = &mut self.buckets[bucket];
        for entry in entries.iter_mut() {
            if entry.is_some_and(|entry| entry.lba == lba) {
                return entry.take().map(|entry| entry.idx);
            }
        }
        None
    }
}

struct Slot {
    lba: Option<Lba>,
    prev: Option<usize>,
    next: Option<usize>,
}

/// 写穿块缓存装饰器：[`read_blocks`] 命中则避免访问 `inner`；未命中合并读取，并在近期
/// 第二次访问时填入 LRU。写入维持 write-through + write-allocate。
struct CacheState {
    block_size: usize,
    capacity: usize,
    data: Vec<u8>,
    map: LbaIndex,
    slots: Vec<Slot>,
    /// 空闲槽下标（仅 `capacity > 0` 时使用）。
    free: Vec<usize>,
    /// 已占用槽组成的双向链表；头部最久未使用，尾部最近使用。
    lru_head: Option<usize>,
    lru_tail: Option<usize>,
    /// LBAs seen recently but not currently resident. A second read admits
    /// the block; evicted residents are also remembered for fast refault.
    recent: RecentIndex,
    #[cfg(feature = "diagnostics")]
    diagnostics: BlockCacheDiagnostics,
}

impl CacheState {
    fn new(block_size: usize, config: BlockCacheConfig) -> Self {
        let capacity = if block_size == 0 { 0 } else { config.capacity_blocks };
        let data = vec![0u8; capacity.checked_mul(block_size).unwrap_or(usize::MAX)];
        let mut slots = Vec::new();
        let mut free = Vec::new();
        if capacity > 0 {
            slots.reserve_exact(capacity);
            for _ in 0..capacity {
                slots.push(Slot {
                    lba: None,
                    prev: None,
                    next: None,
                });
            }
            free.extend((0..capacity).rev());
        }
        Self {
            block_size,
            capacity,
            data,
            map: LbaIndex::new(capacity),
            slots,
            free,
            lru_head: None,
            lru_tail: None,
            recent: RecentIndex::new(capacity),
            #[cfg(feature = "diagnostics")]
            diagnostics: BlockCacheDiagnostics { next_report : DIAGNOSTIC_REPORT_BLOCKS,
                                                 ..BlockCacheDiagnostics::default() },
        }
    }

    #[inline]
    fn slot_data(&self, idx: usize) -> &[u8] {
        let start = idx * self.block_size;
        &self.data[start..start + self.block_size]
    }

    #[inline]
    fn slot_data_mut(&mut self, idx: usize) -> &mut [u8] {
        let start = idx * self.block_size;
        &mut self.data[start..start + self.block_size]
    }

    fn touch_lru(&mut self, idx: usize) {
        if self.lru_tail == Some(idx) {
            return;
        }
        self.detach_lru(idx);
        self.push_lru_back(idx);
    }

    fn detach_lru(&mut self, idx: usize) {
        let prev = self.slots[idx].prev.take();
        let next = self.slots[idx].next.take();
        match prev {
            Some(prev) => self.slots[prev].next = next,
            None => self.lru_head = next,
        }
        match next {
            Some(next) => self.slots[next].prev = prev,
            None => self.lru_tail = prev,
        }
    }

    fn push_lru_back(&mut self, idx: usize) {
        debug_assert!(self.slots[idx].prev.is_none());
        debug_assert!(self.slots[idx].next.is_none());
        self.slots[idx].prev = self.lru_tail;
        if let Some(tail) = self.lru_tail {
            self.slots[tail].next = Some(idx);
        } else {
            self.lru_head = Some(idx);
        }
        self.lru_tail = Some(idx);
    }

    fn alloc_slot(&mut self) -> usize {
        if let Some(idx) = self.free.pop() {
            return idx;
        }
        match self.evict_lru_slot() {
            Ok(idx) => idx,
            Err(e) => {
                log::warn!("[block-cache] alloc_slot evict failed: {e:?}; resetting cache");
                self.reset_cache_invariant();
                self.free.pop()
                    .or_else(|| self.evict_lru_slot().ok())
                    .expect("block cache capacity > 0 but no slot available")
            }
        }
    }

    fn evict_lru_slot(&mut self) -> DriverResult<usize> {
        let Some(idx) = self.lru_head else {
            log::warn!("[block-cache] evict_lru_slot: lru empty");
            return Err(DriverError::IoError);
        };
        self.detach_lru(idx);
        let Some(lba) = self.slots[idx].lba.take() else {
            log::warn!("[block-cache] evict_lru_slot: slot {idx} unoccupied");
            return Err(DriverError::IoError);
        };
        self.map.remove(lba);
        self.recent.insert(lba);
        #[cfg(feature = "diagnostics")]
        {
            self.diagnostics.capacity_evictions += 1;
        }
        Ok(idx)
    }

    #[cfg(feature = "diagnostics")]
    fn note_index_conflict(&mut self) {
        self.diagnostics.index_conflict_evictions += 1;
    }

    #[cfg(feature = "diagnostics")]
    fn note_miss(&mut self) {
        self.diagnostics.miss_blocks += 1;
    }

    /// Install a read-missed block only after the LBA has been observed in
    /// the recent history. First-touch streaming data bypasses the data cache.
    fn admit_read_miss(&mut self, lba: Lba, block: &[u8]) {
        if self.recent.take(lba) {
            #[cfg(feature = "diagnostics")]
            {
                self.diagnostics.ghost_hits += 1;
            }
            self.cache_put_new(lba, block);
        } else {
            self.recent.insert(lba);
        }
    }

    #[cfg(feature = "diagnostics")]
    fn maybe_report_diagnostics(&mut self) {
        let total = self.diagnostics.read_blocks + self.diagnostics.write_blocks;
        if total < self.diagnostics.next_report {
            return;
        }
        self.diagnostics.next_report = total.saturating_add(DIAGNOSTIC_REPORT_BLOCKS);
        log::error!("[cache-diag:block] read_blocks={} hit={} miss={} backend_calls={} \
                     backend_blocks={} write_blocks={} write_alloc={} capacity_evict={} \
                     index_evict={} ghost_hit={} resident={}",
                    self.diagnostics.read_blocks,
                    self.diagnostics.hit_blocks,
                    self.diagnostics.miss_blocks,
                    self.diagnostics.backend_read_calls,
                    self.diagnostics.backend_read_blocks,
                    self.diagnostics.write_blocks,
                    self.diagnostics.write_allocations,
                    self.diagnostics.capacity_evictions,
                    self.diagnostics.index_conflict_evictions,
                    self.diagnostics.ghost_hits,
                    self.capacity.saturating_sub(self.free.len()));
    }

    fn reset_cache_invariant(&mut self) {
        self.map = LbaIndex::new(self.capacity);
        self.recent = RecentIndex::new(self.capacity);
        self.lru_head = None;
        self.lru_tail = None;
        self.free.clear();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.lba = None;
            slot.prev = None;
            slot.next = None;
            self.free.push(i);
        }
    }

    /// 写入或更新缓存中的整块；已存在该 LBA 则原地更新并刷新 LRU。
    fn cache_put(&mut self, lba: Lba, block: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        debug_assert_eq!(block.len(), self.block_size);
        if let Some(idx) = self.map.get(lba) {
            self.recent.take(lba);
            self.slot_data_mut(idx).copy_from_slice(block);
            self.touch_lru(idx);
            return;
        }
        let idx = self.alloc_slot();
        self.recent.take(lba);
        self.slots[idx].lba = Some(lba);
        self.slot_data_mut(idx).copy_from_slice(block);
        if let Some((old_lba, old_idx)) = self.map.insert(lba, idx) {
            if self.slots[old_idx].lba == Some(old_lba) {
                self.detach_lru(old_idx);
                self.slots[old_idx].lba = None;
                self.free.push(old_idx);
                self.recent.insert(old_lba);
                #[cfg(feature = "diagnostics")]
                self.note_index_conflict();
            }
        }
        self.push_lru_back(idx);
    }

    /// 与 [`Self::cache_put`] 相同，但假定调用方已确认该 LBA 不在索引中。
    /// 连续 miss 区间在扫描阶段已经逐个查过索引，可省去第二次查找。
    fn cache_put_new(&mut self, lba: Lba, block: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        debug_assert_eq!(block.len(), self.block_size);
        let idx = self.alloc_slot();
        self.recent.take(lba);
        self.slots[idx].lba = Some(lba);
        self.slot_data_mut(idx).copy_from_slice(block);
        if let Some((old_lba, old_idx)) = self.map.insert(lba, idx) {
            if self.slots[old_idx].lba == Some(old_lba) {
                self.detach_lru(old_idx);
                self.slots[old_idx].lba = None;
                self.free.push(old_idx);
                self.recent.insert(old_lba);
                #[cfg(feature = "diagnostics")]
                self.note_index_conflict();
            }
        }
        self.push_lru_back(idx);
    }

}

/// Concurrent write-through cache. Read misses release the cache metadata lock
/// before entering the backend. A writer generation makes a read retry if it
/// overlapped write-through, preventing stale data from escaping or remaining
/// installed in the cache.
pub struct CachingBlockDevice {
    inner: Arc<dyn BlockDevice>,
    block_size: usize,
    capacity: usize,
    writer: AtomicBool,
    write_generation: AtomicUsize,
    #[cfg(feature = "task-wait")]
    write_wait: Mutex<Option<task::WaitQueue>>,
    state: Mutex<CacheState>,
}

impl CachingBlockDevice {
    pub fn new(inner: Arc<dyn BlockDevice>, config: BlockCacheConfig) -> Self {
        let block_size = inner.block_size();
        let state = CacheState::new(block_size, config);
        Self {
            inner,
            block_size,
            capacity: state.capacity,
            writer: AtomicBool::new(false),
            write_generation: AtomicUsize::new(0),
            #[cfg(feature = "task-wait")]
            write_wait: Mutex::new(None),
            state: Mutex::new(state),
        }
    }

    /// Write-through has no independent dirty state.
    pub fn flush(&self) -> DriverResult<()> { Ok(()) }

    #[cfg(feature = "task-wait")]
    fn write_wait_queue(&self) -> task::WaitQueue {
        let mut wait = self.write_wait.lock();
        *wait.get_or_insert_with(|| task::WaitQueue::new_named("block-cache-write"))
    }

    #[cfg(feature = "task-wait")]
    fn can_sleep() -> bool {
        task::current_task_snapshot().is_some_and(|snapshot| {
            matches!(snapshot.state, task::TaskState::Running)
        })
    }

    fn wait_for_writer(&self) {
        #[cfg(feature = "task-wait")]
        if Self::can_sleep() {
            self.write_wait_queue().wait_current_while(|| self.writer.load(Ordering::Acquire));
            return;
        }
        core::hint::spin_loop();
    }

    fn acquire_writer(&self) {
        'retry: loop {
            if self.writer.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                          .is_ok()
            {
                return;
            }
            self.wait_for_writer();
        }
    }

    fn release_writer(&self) {
        self.write_generation.fetch_add(1, Ordering::Release);
        self.writer.store(false, Ordering::Release);
        #[cfg(feature = "task-wait")]
        {
            if let Some(wait) = *self.write_wait.lock() {
                wait.wake_all();
            }
        }
    }
}

impl BlockDevice for CachingBlockDevice {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn total_blocks(&self) -> Option<u64> {
        self.inner.total_blocks()
    }

    fn read_blocks(&self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        let bs = self.block_size;
        if bs == 0 || buf.len() % bs != 0 {
            return Err(DriverError::InvalidParam);
        }
        if self.capacity == 0 {
            return self.inner.read_blocks(start_block, buf);
        }

        let nblocks = buf.len() / bs;
        'retry: loop {
            let generation = self.write_generation.load(Ordering::Acquire);
            if self.writer.load(Ordering::Acquire) {
                self.wait_for_writer();
                continue;
            }
            #[cfg(feature = "diagnostics")]
            { self.state.lock().diagnostics.read_blocks += nblocks as u64; }
            let base = start_block.0;
            let mut i = 0usize;
            while i < nblocks {
            let mut state = self.state.lock();
            let mut hit_end = i;
            let mut last_hit_idx = None;
            while hit_end < nblocks {
                let lk = Lba(base + hit_end as u64);
                let Some(idx) = state.map.get(lk) else {
                    break;
                };
                buf[hit_end * bs..(hit_end + 1) * bs]
                    .copy_from_slice(state.slot_data(idx));
                #[cfg(feature = "diagnostics")]
                {
                    state.diagnostics.hit_blocks += 1;
                }
                last_hit_idx = Some(idx);
                hit_end += 1;
            }
            if hit_end > i {
                if let Some(idx) = last_hit_idx {
                    state.touch_lru(idx);
                }
                i = hit_end;
                continue;
            }
            let mut j = i + 1;
            #[cfg(feature = "diagnostics")]
            state.note_miss();
            while j < nblocks {
                let lbaj = Lba(base + j as u64);
                if state.map.get(lbaj).is_some() {
                    break;
                }
                #[cfg(feature = "diagnostics")]
                state.note_miss();
                j += 1;
            }
            let run_bytes = (j - i) * bs;
            #[cfg(feature = "diagnostics")]
            {
                state.diagnostics.backend_read_calls += 1;
                state.diagnostics.backend_read_blocks += (j - i) as u64;
            }
            drop(state);
            self.inner.read_blocks(Lba(base + i as u64), &mut buf[i * bs..i * bs + run_bytes])?;
            if self.writer.load(Ordering::Acquire) ||
               self.write_generation.load(Ordering::Acquire) != generation
            {
                continue 'retry;
            }
            let mut state = self.state.lock();
            if self.writer.load(Ordering::Acquire) ||
               self.write_generation.load(Ordering::Acquire) != generation
            {
                drop(state);
                continue 'retry;
            }
            for k in i..j {
                let lk = Lba(base + k as u64);
                if let Some(idx) = state.map.get(lk) {
                    buf[k * bs..(k + 1) * bs].copy_from_slice(state.slot_data(idx));
                    state.touch_lru(idx);
                } else {
                    state.admit_read_miss(lk, &buf[k * bs..(k + 1) * bs]);
                }
            }
            i = j;
            }
            if !self.writer.load(Ordering::Acquire) &&
               self.write_generation.load(Ordering::Acquire) == generation
            {
                #[cfg(feature = "diagnostics")]
                self.state.lock().maybe_report_diagnostics();
                return Ok(());
            }
        }
    }

    fn write_blocks(&self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        let bs = self.block_size;
        if bs == 0 || buf.len() % bs != 0 {
            return Err(DriverError::InvalidParam);
        }
        self.acquire_writer();
        let backend_result = self.inner.write_blocks(start_block, buf);
        if let Err(error) = backend_result {
            self.release_writer();
            return Err(error);
        }
        if self.capacity == 0 {
            self.release_writer();
            return Ok(());
        }
        let nblocks = buf.len() / bs;
        let mut state = self.state.lock();
        #[cfg(feature = "diagnostics")]
        { state.diagnostics.write_blocks += nblocks as u64; }
        for i in 0..nblocks {
            let lba = Lba(start_block.0 + i as u64);
            #[cfg(feature = "diagnostics")]
            if state.map.get(lba).is_none() {
                state.diagnostics.write_allocations += 1;
            }
            state.cache_put(lba, &buf[i * bs..(i + 1) * bs]);
        }
        #[cfg(feature = "diagnostics")]
        state.maybe_report_diagnostics();
        drop(state);
        self.release_writer();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::sync::Arc;
    use std::sync::Mutex;

    struct CountingMem {
        bytes: Mutex<Vec<u8>>,
        reads: Arc<Mutex<usize>>,
        writes: Arc<Mutex<usize>>,
    }

    impl CountingMem {
        fn new(size_blocks: usize, reads: Arc<Mutex<usize>>, writes: Arc<Mutex<usize>>) -> Self {
            Self {
                bytes: Mutex::new(vec![0u8; size_blocks * api_v0::BLOCK_SIZE]),
                reads,
                writes,
            }
        }
    }

    impl BlockDevice for CountingMem {
        fn read_blocks(&self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
            *self.reads.lock().unwrap() += 1;
            let bs = self.block_size();
            if buf.len() % bs != 0 {
                return Err(DriverError::InvalidParam);
            }
            let start = (start_block.0 as usize).checked_mul(bs).ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len()).ok_or(DriverError::InvalidParam)?;
            let bytes = self.bytes.lock().unwrap();
            let src = bytes.get(start..end).ok_or(DriverError::InvalidParam)?;
            buf.copy_from_slice(src);
            Ok(())
        }

        fn write_blocks(&self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
            *self.writes.lock().unwrap() += 1;
            let bs = self.block_size();
            if buf.len() % bs != 0 {
                return Err(DriverError::InvalidParam);
            }
            let start = (start_block.0 as usize).checked_mul(bs).ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len()).ok_or(DriverError::InvalidParam)?;
            let mut bytes = self.bytes.lock().unwrap();
            let dst = bytes.get_mut(start..end).ok_or(DriverError::InvalidParam)?;
            dst.copy_from_slice(buf);
            Ok(())
        }
    }

    #[test]
    fn repeated_read_admits_on_second_miss_then_hits() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(4, reads.clone(), writes.clone()));
        let cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 8 },
        );
        let bs = cache.block_size();
        let mut a = vec![0u8; bs];
        let mut b = vec![0u8; bs];
        let mut c = vec![0u8; bs];
        cache.read_blocks(Lba(1), &mut a).unwrap();
        cache.read_blocks(Lba(1), &mut b).unwrap();
        cache.read_blocks(Lba(1), &mut c).unwrap();
        assert_eq!(*reads.lock().unwrap(), 2);
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(*writes.lock().unwrap(), 0);
    }

    #[test]
    fn contiguous_miss_merged_single_read() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(8, reads.clone(), writes.clone()));
        let cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 8 },
        );
        let bs = cache.block_size();
        let mut buf = vec![0u8; bs * 3];
        cache.read_blocks(Lba(2), &mut buf).unwrap();
        assert_eq!(*reads.lock().unwrap(), 1);
        assert_eq!(cache.state.lock().free.len(), cache.capacity,
                   "first-touch scan must not consume data slots");
    }

    #[test]
    fn contiguous_hit_run_serves_all_from_cache() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(4, reads.clone(), writes.clone()));
        let cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 4 },
        );
        let bs = cache.block_size();
        let mut first = vec![0u8; bs * 2];
        cache.read_blocks(Lba(0), &mut first).unwrap();
        assert_eq!(*reads.lock().unwrap(), 1);

        let before = *reads.lock().unwrap();
        let mut second = vec![0u8; bs * 2];
        cache.read_blocks(Lba(0), &mut second).unwrap();
        assert_eq!(second, first);
        assert_eq!(*reads.lock().unwrap(), before + 1);

        let before_hit = *reads.lock().unwrap();
        cache.read_blocks(Lba(0), &mut second).unwrap();
        assert_eq!(*reads.lock().unwrap(), before_hit);
    }

    #[test]
    fn hit_refreshes_lru_before_eviction() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(4, reads.clone(), writes));
        let cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 2 },
        );
        let mut buf = vec![0u8; cache.block_size()];

        for _ in 0..2 {
            cache.read_blocks(Lba(0), &mut buf).unwrap();
        }
        for _ in 0..2 {
            cache.read_blocks(Lba(1), &mut buf).unwrap();
        }
        cache.read_blocks(Lba(0), &mut buf).unwrap();
        for _ in 0..2 {
            cache.read_blocks(Lba(2), &mut buf).unwrap();
        }
        cache.read_blocks(Lba(0), &mut buf).unwrap();
        assert_eq!(*reads.lock().unwrap(), 6);

        cache.read_blocks(Lba(1), &mut buf).unwrap();
        assert_eq!(*reads.lock().unwrap(), 7);
        cache.read_blocks(Lba(1), &mut buf).unwrap();
        assert_eq!(*reads.lock().unwrap(), 7,
                   "an evicted resident must be readmitted on its first refault");
    }

    #[test]
    fn write_through_updates_existing_cache_line() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(2, reads.clone(), writes.clone()));
        let cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 4 },
        );
        let bs = cache.block_size();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r).unwrap();
        let w = vec![0xabu8; bs];
        cache.write_blocks(Lba(0), &w).unwrap();
        let mut r2 = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r2).unwrap();
        assert_eq!(r2, w);
        assert_eq!(*writes.lock().unwrap(), 1);
        // 命中缓存，不应再触发底层读
        let before = *reads.lock().unwrap();
        cache.read_blocks(Lba(0), &mut r2).unwrap();
        assert_eq!(*reads.lock().unwrap(), before);
    }

    #[test]
    fn write_allocate_then_read_hits_cache() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(8, reads.clone(), writes.clone()));
        let cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 8 },
        );
        let bs = cache.block_size();
        let w = vec![0xcd_u8; bs];
        cache.write_blocks(Lba(5), &w).unwrap();
        assert_eq!(*writes.lock().unwrap(), 1);
        let before = *reads.lock().unwrap();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(5), &mut r).unwrap();
        assert_eq!(r, w);
        assert_eq!(*reads.lock().unwrap(), before);
    }

    #[test]
    fn capacity_zero_passthrough() {
        let reads = Arc::new(Mutex::new(0));
        let inner = Arc::new(CountingMem::new(2, reads.clone(), Arc::new(Mutex::new(0))));
        let cache = CachingBlockDevice::new(inner, BlockCacheConfig { capacity_blocks: 0 });
        let bs = cache.block_size();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r).unwrap();
        cache.read_blocks(Lba(0), &mut r).unwrap();
        assert_eq!(*reads.lock().unwrap(), 2);
    }

    #[test]
    fn lba_index_set_associative_round_trip() {
        let mut index = LbaIndex::new(8);
        index.insert(Lba(7), 3);
        assert_eq!(index.get(Lba(7)), Some(3));
        index.insert(Lba(7), 5);
        assert_eq!(index.get(Lba(7)), Some(5));
        assert_eq!(index.remove(Lba(7)), Some(5));
        assert_eq!(index.get(Lba(7)), None);
    }

    #[test]
    fn lba_index_half_load_absorbs_common_modulo_imbalance() {
        let mut index = LbaIndex::new(16);
        for i in 0..16usize {
            assert_eq!(index.insert(Lba((i * 2) as u64), i), None);
        }
        for i in 0..16usize {
            assert_eq!(index.get(Lba((i * 2) as u64)), Some(i));
        }
    }

    #[test]
    fn lba_index_rotates_victim_on_extreme_collision() {
        let mut index = LbaIndex::new(16);
        for i in 0..LBA_INDEX_WAYS {
            assert_eq!(index.insert(Lba((i * 4) as u64), i), None);
        }
        assert_eq!(index.insert(Lba(32), 32), Some((Lba(0), 0)));
        assert_eq!(index.insert(Lba(36), 36), Some((Lba(4), 1)));
        assert_eq!(index.get(Lba(32)), Some(32));
        assert_eq!(index.get(Lba(36)), Some(36));
    }
}
