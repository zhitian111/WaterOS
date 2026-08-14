//! 块设备写穿（write-through）LRU 缓存：包装任意 [`BlockDevice`]，对上仍实现同一 trait。
//!
//! 连续未命中区间合并为单次底层 [`BlockDevice::read_blocks`]，减少 VirtIO 等后端往返；
//! 读数据采用二次命中准入，避免顺序扫描把一次性块复制进数据缓存。

#![no_std]
extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use api_v0::{BlockDevice, DriverError, DriverResult, Lba};
use wateros_base_config::fs::BLOCK_CACHE_CAPACITY_BLOCKS;

mod manager;
pub use manager::BlockCacheManager;

#[path = "device.rs"]
mod device;
#[path = "index.rs"]
mod index;
pub use device::CachingBlockDevice;
#[cfg(test)]
pub(crate) use index::LBA_INDEX_WAYS;
pub(crate) use index::{LbaIndex, RecentIndex};

/// 缓存容量与策略参数（v1 仅容量）。
#[derive(Debug, Clone, Copy)]
pub struct BlockCacheConfig {
    pub capacity_blocks : usize,
}

impl Default for BlockCacheConfig {
    fn default() -> Self { Self { capacity_blocks : BLOCK_CACHE_CAPACITY_BLOCKS } }
}
#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver/block-cache] self_test begin");
    let config = BlockCacheConfig::default();
    assert!(config.capacity_blocks > 0);
    log::info!("[driver/block-cache] self_test complete");
}
#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::sync::Arc;
    use std::sync::Mutex;

    struct CountingMem {
        bytes : Vec<u8>,
        reads : Arc<Mutex<usize>>,
        writes : Arc<Mutex<usize>>,
    }

    impl CountingMem {
        fn new(size_blocks : usize, reads : Arc<Mutex<usize>>, writes : Arc<Mutex<usize>>) -> Self {
            Self { bytes : vec![0u8; size_blocks * api_v0::BLOCK_SIZE],
                   reads,
                   writes }
        }
    }

    impl BlockDevice for CountingMem {
        fn total_blocks(&self) -> Option<u64> {
            Some((self.bytes.len() / api_v0::BLOCK_SIZE) as u64)
        }

        fn read_blocks(&mut self, start_block : Lba, buf : &mut [u8]) -> DriverResult<()> {
            *self.reads
                 .lock()
                 .unwrap() += 1;
            let bs = self.block_size();
            if buf.len() % bs != 0 {
                return Err(DriverError::InvalidParam);
            }
            let start = (start_block.0 as usize).checked_mul(bs)
                                                .ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len())
                           .ok_or(DriverError::InvalidParam)?;
            let src = self.bytes
                          .get(start..end)
                          .ok_or(DriverError::InvalidParam)?;
            buf.copy_from_slice(src);
            Ok(())
        }

        fn write_blocks(&mut self, start_block : Lba, buf : &[u8]) -> DriverResult<()> {
            *self.writes
                 .lock()
                 .unwrap() += 1;
            let bs = self.block_size();
            if buf.len() % bs != 0 {
                return Err(DriverError::InvalidParam);
            }
            let start = (start_block.0 as usize).checked_mul(bs)
                                                .ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len())
                           .ok_or(DriverError::InvalidParam)?;
            let dst = self.bytes
                          .get_mut(start..end)
                          .ok_or(DriverError::InvalidParam)?;
            dst.copy_from_slice(buf);
            Ok(())
        }

        fn flush(&mut self) -> DriverResult<()> { Ok(()) }
    }

    #[test]
    fn repeated_read_admits_on_second_miss_then_hits() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(4, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 8 });
        let bs = cache.block_size();
        let mut a = vec![0u8; bs];
        let mut b = vec![0u8; bs];
        let mut c = vec![0u8; bs];
        cache.read_blocks(Lba(1), &mut a)
             .unwrap();
        cache.read_blocks(Lba(1), &mut b)
             .unwrap();
        cache.read_blocks(Lba(1), &mut c)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   2);
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(*writes.lock()
                          .unwrap(),
                   0);
    }

    #[test]
    fn contiguous_miss_merged_single_read() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(8, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 8 });
        let bs = cache.block_size();
        let mut buf = vec![0u8; bs * 3];
        cache.read_blocks(Lba(2), &mut buf)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   1);
        assert_eq!(cache.free.len(),
                   cache.capacity,
                   "first-touch scan must not consume data slots");
    }

    #[test]
    fn contiguous_hit_run_serves_all_from_cache() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(4, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 4 });
        let bs = cache.block_size();
        let mut first = vec![0u8; bs * 2];
        cache.read_blocks(Lba(0), &mut first)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   1);

        let before = *reads.lock()
                           .unwrap();
        let mut second = vec![0u8; bs * 2];
        cache.read_blocks(Lba(0), &mut second)
             .unwrap();
        assert_eq!(second, first);
        assert_eq!(*reads.lock()
                         .unwrap(),
                   before + 1);

        let before_hit = *reads.lock()
                               .unwrap();
        cache.read_blocks(Lba(0), &mut second)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   before_hit);
    }

    #[test]
    fn hit_refreshes_lru_before_eviction() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(4, reads.clone(), writes));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 2 });
        let mut buf = vec![0u8; cache.block_size()];

        for _ in 0..2 {
            cache.read_blocks(Lba(0), &mut buf)
                 .unwrap();
        }
        for _ in 0..2 {
            cache.read_blocks(Lba(1), &mut buf)
                 .unwrap();
        }
        cache.read_blocks(Lba(0), &mut buf)
             .unwrap();
        for _ in 0..2 {
            cache.read_blocks(Lba(2), &mut buf)
                 .unwrap();
        }
        cache.read_blocks(Lba(0), &mut buf)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   6);

        cache.read_blocks(Lba(1), &mut buf)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   7);
        cache.read_blocks(Lba(1), &mut buf)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   7,
                   "an evicted resident must be readmitted on its first refault");
    }

    #[test]
    fn write_through_updates_existing_cache_line() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(2, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 4 });
        let bs = cache.block_size();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r)
             .unwrap();
        let w = vec![0xABu8; bs];
        cache.write_blocks(Lba(0), &w)
             .unwrap();
        let mut r2 = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r2)
             .unwrap();
        assert_eq!(r2, w);
        assert_eq!(*writes.lock()
                          .unwrap(),
                   1);
        // 命中缓存，不应再触发底层读
        let before = *reads.lock()
                           .unwrap();
        cache.read_blocks(Lba(0), &mut r2)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   before);
    }

    #[test]
    fn write_allocate_then_read_hits_cache() {
        let reads = Arc::new(Mutex::new(0));
        let writes = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(8, reads.clone(), writes.clone()));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 8 });
        let bs = cache.block_size();
        let w = vec![0xCD_u8; bs];
        cache.write_blocks(Lba(5), &w)
             .unwrap();
        assert_eq!(*writes.lock()
                          .unwrap(),
                   1);
        let before = *reads.lock()
                           .unwrap();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(5), &mut r)
             .unwrap();
        assert_eq!(r, w);
        assert_eq!(*reads.lock()
                         .unwrap(),
                   before);
    }

    #[test]
    fn capacity_zero_passthrough() {
        let reads = Arc::new(Mutex::new(0));
        let inner = Box::new(CountingMem::new(2,
                                              reads.clone(),
                                              Arc::new(Mutex::new(0))));
        let mut cache = CachingBlockDevice::new(inner,
                                                BlockCacheConfig { capacity_blocks : 0 });
        let bs = cache.block_size();
        let mut r = vec![0u8; bs];
        cache.read_blocks(Lba(0), &mut r)
             .unwrap();
        cache.read_blocks(Lba(0), &mut r)
             .unwrap();
        assert_eq!(*reads.lock()
                         .unwrap(),
                   2);
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
            assert_eq!(index.insert(Lba((i * 2) as u64), i),
                       None);
        }
        for i in 0..16usize {
            assert_eq!(index.get(Lba((i * 2) as u64)), Some(i));
        }
    }

    #[test]
    fn lba_index_rotates_victim_on_extreme_collision() {
        let mut index = LbaIndex::new(16);
        for i in 0..LBA_INDEX_WAYS {
            assert_eq!(index.insert(Lba((i * 4) as u64), i),
                       None);
        }
        assert_eq!(index.insert(Lba(32), 32),
                   Some((Lba(0), 0)));
        assert_eq!(index.insert(Lba(36), 36),
                   Some((Lba(4), 1)));
        assert_eq!(index.get(Lba(32)), Some(32));
        assert_eq!(index.get(Lba(36)), Some(36));
    }
}
