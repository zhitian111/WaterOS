//! 块设备写穿（write-through）LRU 缓存：包装任意 [`BlockDevice`]，对上仍实现同一 trait。
//!
//! 连续未命中区间合并为单次底层 [`BlockDevice::read_blocks`]，减少 VirtIO 等后端往返。

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
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

struct Slot {
    lba: Option<Lba>,
    data: Vec<u8>,
}

/// 写穿块缓存装饰器：[`read_blocks`] 命中则避免访问 `inner`；未命中合并读入并填入 LRU。
pub struct CachingBlockDevice {
    inner: Box<dyn BlockDevice + Send>,
    block_size: usize,
    capacity: usize,
    map: BTreeMap<Lba, usize>,
    slots: Vec<Slot>,
    /// 空闲槽下标（仅 `capacity > 0` 时使用）。
    free: Vec<usize>,
    /// 已占用槽的 LRU 顺序：前部最久未使用。
    lru: VecDeque<usize>,
}

impl CachingBlockDevice {
    /// 用给定配置包装 `inner`；从 `inner` 读取 [`BlockDevice::block_size`] 并预分配槽位缓冲。
    pub fn new(inner: Box<dyn BlockDevice + Send>, config: BlockCacheConfig) -> Self {
        let block_size = inner.block_size();
        let capacity = if block_size == 0 { 0 } else { config.capacity_blocks };
        let mut slots = Vec::new();
        let mut free = Vec::new();
        if capacity > 0 {
            slots.reserve_exact(capacity);
            for _ in 0..capacity {
                slots.push(Slot {
                    lba: None,
                    data: vec![0u8; block_size],
                });
            }
            free.extend((0..capacity).rev());
        }
        Self {
            inner,
            block_size,
            capacity,
            map: BTreeMap::new(),
            slots,
            free,
            lru: VecDeque::new(),
        }
    }

    /// 将脏缓存写回底层（写穿下为 no-op）；保留接口供将来 write-back 或测试钩子使用。
    pub fn flush(&mut self) -> DriverResult<()> {
        let _ = &mut self.inner;
        Ok(())
    }

    fn touch_lru(&mut self, idx: usize) {
        if let Some(p) = self.lru.iter().position(|&x| x == idx) {
            self.lru.remove(p);
        }
        self.lru.push_back(idx);
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
        let Some(idx) = self.lru.pop_front() else {
            log::warn!("[block-cache] evict_lru_slot: lru empty");
            return Err(DriverError::IoError);
        };
        let Some(lba) = self.slots[idx].lba.take() else {
            log::warn!("[block-cache] evict_lru_slot: slot {idx} unoccupied");
            return Err(DriverError::IoError);
        };
        self.map.remove(&lba);
        Ok(idx)
    }

    fn reset_cache_invariant(&mut self) {
        self.map.clear();
        self.lru.clear();
        self.free.clear();
        for (i, slot) in self.slots.iter_mut().enumerate() {
            slot.lba = None;
            self.free.push(i);
        }
    }

    /// 写入或更新缓存中的整块；已存在该 LBA 则原地更新并刷新 LRU。
    fn cache_put(&mut self, lba: Lba, block: &[u8]) {
        if self.capacity == 0 {
            return;
        }
        debug_assert_eq!(block.len(), self.block_size);
        if let Some(&idx) = self.map.get(&lba) {
            self.slots[idx].data.copy_from_slice(block);
            self.touch_lru(idx);
            return;
        }
        let idx = self.alloc_slot();
        self.slots[idx].lba = Some(lba);
        self.slots[idx].data.copy_from_slice(block);
        self.map.insert(lba, idx);
        self.lru.push_back(idx);
    }

    fn cache_copy_out(&mut self, lba: Lba, dst: &mut [u8]) -> bool {
        if self.capacity == 0 {
            return false;
        }
        let Some(&idx) = self.map.get(&lba) else {
            return false;
        };
        dst.copy_from_slice(&self.slots[idx].data);
        self.touch_lru(idx);
        true
    }
}

impl BlockDevice for CachingBlockDevice {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn total_blocks(&self) -> Option<u64> {
        self.inner.total_blocks()
    }

    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        let bs = self.block_size;
        if bs == 0 || buf.len() % bs != 0 {
            return Err(DriverError::InvalidParam);
        }
        if self.capacity == 0 {
            return self.inner.read_blocks(start_block, buf);
        }

        let nblocks = buf.len() / bs;
        let base = start_block.0;
        let mut i = 0usize;
        while i < nblocks {
            let lba = Lba(base + i as u64);
            let row = &mut buf[i * bs..(i + 1) * bs];
            if self.cache_copy_out(lba, row) {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < nblocks {
                let lbaj = Lba(base + j as u64);
                if self.map.contains_key(&lbaj) {
                    break;
                }
                j += 1;
            }
            let run_bytes = (j - i) * bs;
            self.inner.read_blocks(Lba(base + i as u64), &mut buf[i * bs..i * bs + run_bytes])?;
            for k in i..j {
                let lk = Lba(base + k as u64);
                self.cache_put(lk, &buf[k * bs..(k + 1) * bs]);
            }
            i = j;
        }
        Ok(())
    }

    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        let bs = self.block_size;
        if bs == 0 || buf.len() % bs != 0 {
            return Err(DriverError::InvalidParam);
        }
        self.inner.write_blocks(start_block, buf)?;
        if self.capacity == 0 {
            return Ok(());
        }
        let nblocks = buf.len() / bs;
        for i in 0..nblocks {
            let lba = Lba(start_block.0 + i as u64);
            self.cache_put(lba, &buf[i * bs..(i + 1) * bs]);
        }
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
        bytes: Vec<u8>,
        reads: Arc<Mutex<usize>>,
        writes: Arc<Mutex<usize>>,
    }

    impl CountingMem {
        fn new(size_blocks: usize, reads: Arc<Mutex<usize>>, writes: Arc<Mutex<usize>>) -> Self {
            Self {
                bytes: vec![0u8; size_blocks * api_v0::BLOCK_SIZE],
                reads,
                writes,
            }
        }
    }

    impl BlockDevice for CountingMem {
        fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
            *self.reads.lock().unwrap() += 1;
            let bs = self.block_size();
            if buf.len() % bs != 0 {
                return Err(DriverError::InvalidParam);
            }
            let start = (start_block.0 as usize).checked_mul(bs).ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len()).ok_or(DriverError::InvalidParam)?;
            let src = self.bytes.get(start..end).ok_or(DriverError::InvalidParam)?;
            buf.copy_from_slice(src);
            Ok(())
        }

        fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
            *self.writes.lock().unwrap() += 1;
            let bs = self.block_size();
            if buf.len() % bs != 0 {
                return Err(DriverError::InvalidParam);
            }
            let start = (start_block.0 as usize).checked_mul(bs).ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len()).ok_or(DriverError::InvalidParam)?;
            let dst = self.bytes.get_mut(start..end).ok_or(DriverError::InvalidParam)?;
            dst.copy_from_slice(buf);
            Ok(())
        }
    }

    #[test]
    fn repeated_read_same_block_uses_one_backend_read() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(4, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 8 },
        );
        let bs = cache.block_size();
        let mut a = vec![0u8; bs];
        let mut b = vec![0u8; bs];
        cache.read_blocks(Lba(1), &mut a).unwrap();
        cache.read_blocks(Lba(1), &mut b).unwrap();
        assert_eq!(*reads.lock().unwrap(), 1);
        assert_eq!(*writes.lock().unwrap(), 0);
    }

    #[test]
    fn contiguous_miss_merged_single_read() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(8, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(
            inner,
            BlockCacheConfig { capacity_blocks: 8 },
        );
        let bs = cache.block_size();
        let mut buf = vec![0u8; bs * 3];
        cache.read_blocks(Lba(2), &mut buf).unwrap();
        assert_eq!(*reads.lock().unwrap(), 1);
    }

    #[test]
    fn write_through_updates_existing_cache_line() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(2, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(
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
        let inner = Box::new(CountingMem::new(8, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(
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
        let inner = Box::new(CountingMem::new(2, reads.clone(), Arc::new(Mutex::new(0))));
        let mut cache = CachingBlockDevice::new(inner, BlockCacheConfig { capacity_blocks: 0 });
        let bs = cache.block_size();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r).unwrap();
        cache.read_blocks(Lba(0), &mut r).unwrap();
        assert_eq!(*reads.lock().unwrap(), 2);
    }
}
