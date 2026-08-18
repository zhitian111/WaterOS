//! LBA 哈希索引和二次命中历史表；只保存元数据，不保存数据块正文。

use super::*;

#[derive(Clone, Copy)]
pub(crate) struct LbaIndexEntry {
    lba : Lba,
    idx : usize,
}

pub(crate) struct LbaIndex {
    buckets : Vec<[Option<LbaIndexEntry>; LBA_INDEX_WAYS]>,
    next_victim : Vec<u8>,
}

pub(crate) const LBA_INDEX_WAYS : usize = 8;

const RECENT_INDEX_WAYS : usize = 4;
#[cfg(feature = "diagnostics")]
const DIAGNOSTIC_REPORT_BLOCKS : u64 = 1 << 20;

/// 用于二次命中读准入的近似 miss/refault 历史。
///
/// 表只保存元数据且允许替换：误报阴性只会延迟一次准入；完整 LBA 比较保证不会误报阳性。
pub(crate) struct RecentIndex {
    buckets : Vec<[Option<Lba>; RECENT_INDEX_WAYS]>,
    next : Vec<u8>,
}

impl RecentIndex {
    pub(crate) fn new(capacity : usize) -> Self {
        // 历史项数量按数据槽两倍配置，使占用率不超过约 50%，避免冲突挤满准入索引。
        let bucket_count = capacity.div_ceil(RECENT_INDEX_WAYS / 2)
                                   .max(1);
        Self { buckets : vec![[None; RECENT_INDEX_WAYS]; bucket_count],
               next : vec![0; bucket_count] }
    }

    pub(crate) fn bucket(&self, lba : Lba) -> usize { (lba.0 as usize) % self.buckets.len() }

    pub(crate) fn take(&mut self, lba : Lba) -> bool {
        let bucket = self.bucket(lba);
        for entry in &mut self.buckets[bucket] {
            if *entry == Some(lba) {
                *entry = None;
                return true;
            }
        }
        false
    }

    pub(crate) fn insert(&mut self, lba : Lba) {
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
    pub(crate) fn new(capacity : usize) -> Self {
        // 即使所有数据槽都占用，索引负载也保持在约 50% 以下，避免哈希不均导致
        // 数据缓存仍有空间却频繁发生冲突淘汰。
        let bucket_count = capacity
                               .div_ceil(LBA_INDEX_WAYS / 2)
                               .max(1);
        Self { buckets : vec![[None; LBA_INDEX_WAYS]; bucket_count],
               next_victim : vec![0; bucket_count] }
    }

    pub(crate) fn bucket(&self, lba : Lba) -> usize {
        (lba.0 as usize) % self.buckets.len()
    }

    pub(crate) fn get(&self, lba : Lba) -> Option<usize> {
        let bucket = self.bucket(lba);
        self.buckets[bucket]
            .iter()
            .find_map(|entry| {
                entry.and_then(|entry| {
                    (entry.lba == lba).then_some(entry.idx)
                })
            })
    }

    pub(crate) fn insert(&mut self, lba : Lba, idx : usize) -> Option<(Lba, usize)> {
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

    pub(crate) fn remove(&mut self, lba : Lba) -> Option<usize> {
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
