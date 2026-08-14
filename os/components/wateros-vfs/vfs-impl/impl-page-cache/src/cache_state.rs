use super::*;

// 本结构代码由AI完成
pub(crate) struct PageFrame {
    pub(crate) key : Option<(FileCacheKey, u64)>,
    pub(crate) dirty : bool,
    pub(crate) version : u64,
    pub(crate) lru_prev : Option<usize>,
    pub(crate) lru_next : Option<usize>,
    pub(crate) lru_class : Option<LruClass>,
    #[cfg(feature = "diagnostics")]
    pub(crate) referenced : bool,
    #[cfg(feature = "diagnostics")]
    pub(crate) prefetched : bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LruClass {
    Clean,
    Dirty,
}

// 本结构代码由AI完成
pub(crate) struct GlobalCacheState {
    pub(crate) capacity : usize,
    /// 所有页帧 payload 共用连续池，避免每槽一次 4 KiB 堆分配放大 TLSF 锁竞争与碎片。
    pub(crate) data : Vec<u8>,
    pub(crate) frames : Vec<PageFrame>,
    pub(crate) index : BTreeMap<(FileCacheKey, u64), usize>,
    pub(crate) clean_lru_head : Option<usize>,
    pub(crate) clean_lru_tail : Option<usize>,
    pub(crate) dirty_lru_head : Option<usize>,
    pub(crate) dirty_lru_tail : Option<usize>,
    pub(crate) free : Vec<usize>,
    pub(crate) next_version : u64,
    #[cfg(feature = "diagnostics")]
    pub(crate) diagnostics : PageCacheDiagnostics,
}

impl GlobalCacheState {
// 本方法代码由AI完成
    pub(crate) fn new() -> Self { Self::with_capacity(FILE_PAGE_CACHE_CAPACITY) }

    pub(crate) fn with_capacity(cap : usize) -> Self {
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
                                        lru_class : None,
                                        #[cfg(feature = "diagnostics")]
                                        referenced : false,
                                        #[cfg(feature = "diagnostics")]
                                        prefetched : false });
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
               next_version : 0,
               #[cfg(feature = "diagnostics")]
               diagnostics : PageCacheDiagnostics { next_report : DIAGNOSTIC_REPORT_LOOKUPS,
                                                    ..PageCacheDiagnostics::default() } }
    }

    #[cfg(feature = "diagnostics")]
    pub(crate) fn note_lookup(&mut self, idx : Option<usize>, source : InstallSource) {
        match source {
            InstallSource::Demand => self.diagnostics.demand_lookups += 1,
            InstallSource::Prefetch => self.diagnostics.prefetch_lookups += 1,
        }
        if let Some(idx) = idx {
            self.diagnostics.hits += 1;
            self.frames[idx].referenced = true;
            if source == InstallSource::Demand && self.frames[idx].prefetched {
                self.frames[idx].prefetched = false;
                self.diagnostics.prefetch_uses += 1;
            }
        } else {
            self.diagnostics.misses += 1;
        }
        self.maybe_report_diagnostics();
    }

    #[cfg(feature = "diagnostics")]
    pub(crate) fn note_install(&mut self, idx : usize, source : InstallSource) {
        self.diagnostics.installs += 1;
        if source == InstallSource::Prefetch {
            self.diagnostics.prefetch_installs += 1;
        }
        self.frames[idx].referenced = false;
        self.frames[idx].prefetched = source == InstallSource::Prefetch;
    }

    #[cfg(feature = "diagnostics")]
    pub(crate) fn maybe_report_diagnostics(&mut self) {
        let total = self.diagnostics.demand_lookups + self.diagnostics.prefetch_lookups;
        if total < self.diagnostics.next_report {
            return;
        }
        self.diagnostics.next_report = total.saturating_add(DIAGNOSTIC_REPORT_LOOKUPS);
        log::error!("[cache-diag:page] demand={} prefetch={} hit={} miss={} installs={} \
                     duplicate_load={} clean_evict={} dirty_evict={} unused_evict={} \
                     prefetch_install={} prefetch_use={} resident={}",
                    self.diagnostics.demand_lookups,
                    self.diagnostics.prefetch_lookups,
                    self.diagnostics.hits,
                    self.diagnostics.misses,
                    self.diagnostics.installs,
                    self.diagnostics.duplicate_loads,
                    self.diagnostics.clean_evictions,
                    self.diagnostics.dirty_evictions,
                    self.diagnostics.unused_evictions,
                    self.diagnostics.prefetch_installs,
                    self.diagnostics.prefetch_uses,
                    self.index.len());
    }

    #[inline]
    pub(crate) fn page_data(&self, idx : usize) -> &[u8] {
        let start = idx * FILE_PAGE_SIZE;
        &self.data[start..start + FILE_PAGE_SIZE]
    }

    #[inline]
    pub(crate) fn page_data_mut(&mut self, idx : usize) -> &mut [u8] {
        let start = idx * FILE_PAGE_SIZE;
        &mut self.data[start..start + FILE_PAGE_SIZE]
    }

    pub(crate) fn lru_ends(&self, class : LruClass) -> (Option<usize>, Option<usize>) {
        match class {
            LruClass::Clean => (self.clean_lru_head, self.clean_lru_tail),
            LruClass::Dirty => (self.dirty_lru_head, self.dirty_lru_tail),
        }
    }

    pub(crate) fn set_lru_head(&mut self, class : LruClass, head : Option<usize>) {
        match class {
            LruClass::Clean => self.clean_lru_head = head,
            LruClass::Dirty => self.dirty_lru_head = head,
        }
    }

    pub(crate) fn set_lru_tail(&mut self, class : LruClass, tail : Option<usize>) {
        match class {
            LruClass::Clean => self.clean_lru_tail = tail,
            LruClass::Dirty => self.dirty_lru_tail = tail,
        }
    }

    pub(crate) fn remove_from_lru(&mut self, idx : usize) {
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

    pub(crate) fn push_lru_back(&mut self, idx : usize, class : LruClass) {
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
    pub(crate) fn touch_lru(&mut self, idx : usize) {
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

    pub(crate) fn pop_lru_front(&mut self, class : LruClass) -> Option<usize> {
        let (head, _) = self.lru_ends(class);
        if let Some(idx) = head {
            self.remove_from_lru(idx);
        }
        head
    }

// 本方法代码由AI完成
    pub(crate) fn pop_free_or_lru_index(&mut self) -> Option<usize> {
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
    pub(crate) fn detach_slot_for_reuse(&mut self,
                             idx : usize,
                             was_dirty : bool)
                             -> Option<((FileCacheKey, u64), Vec<u8>, u64)> {
        #[cfg(feature = "diagnostics")]
        if self.frames[idx].key.is_some() {
            if was_dirty {
                self.diagnostics.dirty_evictions += 1;
            } else {
                self.diagnostics.clean_evictions += 1;
            }
            if !self.frames[idx].referenced {
                self.diagnostics.unused_evictions += 1;
            }
        }
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
        #[cfg(feature = "diagnostics")]
        {
            self.frames[idx].referenced = false;
            self.frames[idx].prefetched = false;
        }
        dirty_data
    }

    pub(crate) fn mark_dirty(&mut self, idx : usize) -> u64 {
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

    pub(crate) fn mark_clean(&mut self, idx : usize) {
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
    pub(crate) fn return_detached_slot(&mut self, idx : usize) {
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
    pub(crate) fn clear_in_place(&mut self) {
        for frame in self.frames
                         .iter_mut()
        {
            frame.key = None;
            frame.dirty = false;
            frame.version = 0;
            frame.lru_prev = None;
            frame.lru_next = None;
            frame.lru_class = None;
            #[cfg(feature = "diagnostics")]
            {
                frame.referenced = false;
                frame.prefetched = false;
            }
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
    pub(crate) fn assert_lru_invariants(&self) {
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


