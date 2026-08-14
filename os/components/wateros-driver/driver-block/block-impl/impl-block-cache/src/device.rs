use super::*;


pub(crate) struct Slot {
    lba: Option<Lba>,
    prev: Option<usize>,
    next: Option<usize>,
}

/// 写穿块缓存装饰器：[`read_blocks`] 命中则避免访问 `inner`；未命中合并读取，并在近期
/// 第二次访问时填入 LRU。写入维持 write-through + write-allocate。
pub struct CachingBlockDevice {
    pub(crate) inner: Box<dyn BlockDevice + Send>,
    pub(crate) block_size: usize,
    pub(crate) capacity: usize,
    pub(crate) data: Vec<u8>,
    pub(crate) map: LbaIndex,
    pub(crate) slots: Vec<Slot>,
    /// 空闲槽下标（仅 `capacity > 0` 时使用）。
    pub(crate) free: Vec<usize>,
    /// 已占用槽组成的双向链表；头部最久未使用，尾部最近使用。
    pub(crate) lru_head: Option<usize>,
    pub(crate) lru_tail: Option<usize>,
    /// LBAs seen recently but not currently resident. A second read admits
    /// the block; evicted residents are also remembered for fast refault.
    pub(crate) recent: RecentIndex,
    #[cfg(feature = "diagnostics")]
    pub(crate) diagnostics: BlockCacheDiagnostics,
}

impl CachingBlockDevice {
    /// 用给定配置包装 `inner`；从 `inner` 读取 [`BlockDevice::block_size`] 并预分配槽位缓冲。
    pub fn new(inner: Box<dyn BlockDevice + Send>, config: BlockCacheConfig) -> Self {
        let block_size = inner.block_size();
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
            inner,
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
    pub(crate) fn slot_data(&self, idx: usize) -> &[u8] {
        let start = idx * self.block_size;
        &self.data[start..start + self.block_size]
    }

    #[inline]
    pub(crate) fn slot_data_mut(&mut self, idx: usize) -> &mut [u8] {
        let start = idx * self.block_size;
        &mut self.data[start..start + self.block_size]
    }

    /// 将脏缓存写回底层（写穿下为 no-op）；保留接口供将来 write-back 或测试钩子使用。
    pub fn flush(&mut self) -> DriverResult<()> {
        let _ = &mut self.inner;
        Ok(())
    }

    fn validate_io_range(&self, start_block: Lba, byte_len: usize) -> DriverResult<usize> {
        if self.block_size == 0 || byte_len % self.block_size != 0 {
            return Err(DriverError::InvalidParam);
        }
        let block_count = byte_len / self.block_size;
        let count = u64::try_from(block_count).map_err(|_| DriverError::OutOfRange)?;
        let end = start_block.0.checked_add(count).ok_or(DriverError::OutOfRange)?;
        if self.inner.total_blocks().is_some_and(|total| end > total) {
            return Err(DriverError::OutOfRange);
        }
        Ok(block_count)
    }

    pub(crate) fn touch_lru(&mut self, idx: usize) {
        if self.lru_tail == Some(idx) {
            return;
        }
        self.detach_lru(idx);
        self.push_lru_back(idx);
    }

    pub(crate) fn detach_lru(&mut self, idx: usize) {
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

    pub(crate) fn push_lru_back(&mut self, idx: usize) {
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

    pub(crate) fn alloc_slot(&mut self) -> usize {
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

    pub(crate) fn evict_lru_slot(&mut self) -> DriverResult<usize> {
        let Some(idx) = self.lru_head else {
            log::warn!("[block-cache] evict_lru_slot: lru empty");
            return Err(DriverError::Protocol);
        };
        self.detach_lru(idx);
        let Some(lba) = self.slots[idx].lba.take() else {
            log::warn!("[block-cache] evict_lru_slot: slot {idx} unoccupied");
            return Err(DriverError::Protocol);
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
    pub(crate) fn note_index_conflict(&mut self) {
        self.diagnostics.index_conflict_evictions += 1;
    }

    #[cfg(feature = "diagnostics")]
    pub(crate) fn note_miss(&mut self) {
        self.diagnostics.miss_blocks += 1;
    }

    /// Install a read-missed block only after the LBA has been observed in
    /// the recent history. First-touch streaming data bypasses the data cache.
    pub(crate) fn admit_read_miss(&mut self, lba: Lba, block: &[u8]) {
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
    pub(crate) fn maybe_report_diagnostics(&mut self) {
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

    pub(crate) fn reset_cache_invariant(&mut self) {
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
    pub(crate) fn cache_put(&mut self, lba: Lba, block: &[u8]) {
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
    pub(crate) fn cache_put_new(&mut self, lba: Lba, block: &[u8]) {
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

impl BlockDevice for CachingBlockDevice {
    fn block_size(&self) -> usize {
        self.block_size
    }

    fn total_blocks(&self) -> Option<u64> {
        self.inner.total_blocks()
    }

    fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> DriverResult<()> {
        let nblocks = self.validate_io_range(start_block, buf.len())?;
        if nblocks == 0 {
            return Ok(());
        }
        let bs = self.block_size;
        if self.capacity == 0 {
            return self.inner.read_blocks(start_block, buf);
        }

        #[cfg(feature = "diagnostics")]
        {
            self.diagnostics.read_blocks += nblocks as u64;
        }
        let base = start_block.0;
        let mut i = 0usize;
        while i < nblocks {
            let mut hit_end = i;
            let mut last_hit_idx = None;
            while hit_end < nblocks {
                let lk = Lba(base + hit_end as u64);
                let Some(idx) = self.map.get(lk) else {
                    break;
                };
                buf[hit_end * bs..(hit_end + 1) * bs]
                    .copy_from_slice(self.slot_data(idx));
                #[cfg(feature = "diagnostics")]
                {
                    self.diagnostics.hit_blocks += 1;
                }
                last_hit_idx = Some(idx);
                hit_end += 1;
            }
            if hit_end > i {
                if let Some(idx) = last_hit_idx {
                    self.touch_lru(idx);
                }
                i = hit_end;
                continue;
            }
            let mut j = i + 1;
            #[cfg(feature = "diagnostics")]
            self.note_miss();
            while j < nblocks {
                let lbaj = Lba(base + j as u64);
                if self.map.get(lbaj).is_some() {
                    break;
                }
                #[cfg(feature = "diagnostics")]
                self.note_miss();
                j += 1;
            }
            let run_bytes = (j - i) * bs;
            #[cfg(feature = "diagnostics")]
            {
                self.diagnostics.backend_read_calls += 1;
                self.diagnostics.backend_read_blocks += (j - i) as u64;
            }
            self.inner.read_blocks(Lba(base + i as u64), &mut buf[i * bs..i * bs + run_bytes])?;
            for k in i..j {
                let lk = Lba(base + k as u64);
                self.admit_read_miss(lk, &buf[k * bs..(k + 1) * bs]);
            }
            i = j;
        }
        #[cfg(feature = "diagnostics")]
        self.maybe_report_diagnostics();
        Ok(())
    }

    fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> DriverResult<()> {
        let nblocks = self.validate_io_range(start_block, buf.len())?;
        if nblocks == 0 {
            return Ok(());
        }
        let bs = self.block_size;
        self.inner.write_blocks(start_block, buf)?;
        if self.capacity == 0 {
            return Ok(());
        }
        #[cfg(feature = "diagnostics")]
        {
            self.diagnostics.write_blocks += nblocks as u64;
        }
        for i in 0..nblocks {
            let lba = Lba(start_block.0 + i as u64);
            #[cfg(feature = "diagnostics")]
            if self.map.get(lba).is_none() {
                self.diagnostics.write_allocations += 1;
            }
            self.cache_put(lba, &buf[i * bs..(i + 1) * bs]);
        }
        #[cfg(feature = "diagnostics")]
        self.maybe_report_diagnostics();
        Ok(())
    }
}
