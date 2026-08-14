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

/// Approximate recent-miss/refault history used for two-hit read admission.
///
/// The table deliberately stores no data and tolerates replacement: a false
/// negative only delays admission until another read, while false positives
/// are impossible because the complete LBA is compared.
pub(crate) struct RecentIndex {
    buckets : Vec<[Option<Lba>; RECENT_INDEX_WAYS]>,
    next : Vec<u8>,
}

impl RecentIndex {
    pub(crate) fn new(capacity : usize) -> Self {
        // Keep at most 50% occupancy when the history contains twice as many
        // entries as data slots. This avoids recreating the old full-table
        // conflict problem in the admission index.
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
        // Keep index occupancy at or below 50% when all data slots are live.
        // The old 100%-full table turned ordinary hash imbalance into millions
        // of conflict evictions even though the data cache itself had room.
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
