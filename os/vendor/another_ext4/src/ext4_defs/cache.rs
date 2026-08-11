#![cfg(feature = "block_cache")]

use crate::constants::*;
use crate::prelude::*;
use crate::Block;
use crate::BlockDevice;
use spin::Mutex;

/// Write-back cache slot.
#[derive(Debug, Clone, Default)]
struct CacheSlot {
    /// Valid flag.
    valid: bool,
    /// Dirty flag.
    dirty: bool,
    /// Previous slot in the LRU list.
    prev: u8,
    /// Next slot in the LRU list.
    next: u8,
    /// Block data.
    block: Block,
}

/// Associative cache set.
#[derive(Debug, Clone)]
struct CacheSet {
    /// `CACHE_ASSOC`-way-associative slots.
    slots: [CacheSlot; CACHE_ASSOC],
    /// Head of the LRU list.
    head: u8,
}

impl CacheSet {
    /// Initialize the cache set. Initialize in heap to avoid stack overflow.
    fn new() -> Self {
        let mut set = CacheSet {
            slots: core::array::from_fn(|_| CacheSlot::default()),
            head: CACHE_ASSOC as u8 - 1,
        };
        for i in 1..CACHE_ASSOC as u8 {
            set.link(i - 1, i);
        }
        set.link(CACHE_ASSOC as u8 - 1, 0);
        set
    }

    /// Link 2 LRU list nodes.
    fn link(&mut self, prev: u8, cur: u8) {
        self.slots[prev as usize].next = cur;
        self.slots[cur as usize].prev = prev;
    }

    /// Access a block in the cache set.
    fn access(&mut self, block_id: PBlockId) -> usize {
        // Check if there is a slot allocated for the block
        let slot = self
            .slots
            .iter()
            .position(|b| b.valid && b.block.id == block_id);
        if let Some(slot) = slot {
            // If yes, set head as slot_id
            if self.head != slot as u8 {
                self.link(self.slots[slot].prev, self.slots[slot].next);
                self.link(self.slots[self.head as usize].prev, slot as u8);
                self.link(slot as u8, self.head);
                self.head = slot as u8;
            }
            slot
        } else {
            // If not, head goes 1 step forward to reach the last slot
            self.head = self.slots[self.head as usize].prev;
            self.head as usize
        }
    }
}

/// LRU Write-back Block Cache.
pub struct BlockCache {
    /// Block cache allocated on the heap.
    cache: Arc<Mutex<[CacheSet; CACHE_SIZE]>>,
    /// The underlying block device.
    block_dev: Arc<dyn BlockDevice>,
}

impl BlockCache {
    /// Create a new block cache on a block device.
    pub fn new(block_dev: Arc<dyn BlockDevice>) -> Self {
        let cache: Vec<CacheSet> = (0..CACHE_SIZE).map(|_| CacheSet::new()).collect();
        Self {
            cache: Arc::new(Mutex::new(cache.try_into().unwrap())),
            block_dev,
        }
    }

    /// Read a block.
    pub fn read_block(&self, block_id: PBlockId) -> Block {
        debug!("Reading block {}", block_id);
        let set_id = block_id as usize % CACHE_SIZE;
        let mut cache = self.cache.lock();
        let slot_id = cache[set_id].access(block_id) as usize;
        let slot = &mut cache[set_id].slots[slot_id];
        // Check block id
        if slot.valid && slot.block.id == block_id {
            // Cache hit
            return slot.block.clone();
        } else {
            // Cache miss
            if slot.valid && slot.dirty {
                // Write back Dirty block
                self.block_dev.write_block(&slot.block);
                slot.dirty = false;
            }
            // Read block from disk
            debug!("Loading block {} from disk", block_id);
            let block = self.block_dev.read_block(block_id);
            slot.block = block.clone();
            slot.valid = true;
            return block;
        }
    }

    /// Read a physically contiguous run with one lower-device request while
    /// preserving cached (including dirty) blocks as the authoritative view.
    pub fn read_blocks(&self, start_block: PBlockId, buf: &mut [u8]) {
        assert_eq!(buf.len() % BLOCK_SIZE, 0);
        if buf.is_empty() {
            return;
        }

        // Do not hold the cache lock across device I/O. Existing cache hits
        // are overlaid below, so dirty write-back data cannot be exposed as
        // stale disk content.
        self.block_dev.read_blocks(start_block, buf);

        let mut cache = self.cache.lock();
        for (index, chunk) in buf.chunks_exact_mut(BLOCK_SIZE).enumerate() {
            let block_id = start_block + index as PBlockId;
            let set_id = block_id as usize % CACHE_SIZE;
            let slot_id = cache[set_id].access(block_id) as usize;
            let slot = &mut cache[set_id].slots[slot_id];
            if slot.valid && slot.block.id == block_id {
                chunk.copy_from_slice(slot.block.data());
                continue;
            }
            if slot.valid && slot.dirty {
                self.block_dev.write_block(&slot.block);
            }
            let mut data = Box::new([0u8; BLOCK_SIZE]);
            data.copy_from_slice(chunk);
            slot.block = Block::new(block_id, data);
            slot.valid = true;
            slot.dirty = false;
        }
    }

    /// Write a block. (Write-Allocate)
    pub fn write_block(&self, block: &Block) {
        debug!("Writing block {}", block.id);
        let set_id = block.id as usize % CACHE_SIZE;
        let mut cache = self.cache.lock();
        let slot_id = cache[set_id].access(block.id) as usize;
        let slot = &mut cache[set_id].slots[slot_id];
        // Check block id
        if slot.valid && slot.block.id == block.id {
            // Cache hit
            slot.block = block.clone();
            slot.dirty = true;
        } else {
            // Cache miss
            if slot.valid && slot.dirty {
                // Write back Dirty block
                self.block_dev.write_block(&slot.block);
                slot.dirty = false;
            }
            // Write allocate
            slot.block = block.clone();
            slot.valid = true;
            slot.dirty = true;
        }
    }

    /// Flush a block to disk.
    #[allow(unused)]
    pub fn flush(&self, block_id: PBlockId) {
        let mut cache = self.cache.lock();
        let set_id = block_id as usize % CACHE_SIZE;
        let slot_id = cache[set_id].access(block_id) as usize;
        let slot = &mut cache[set_id].slots[slot_id];
        if slot.valid && slot.dirty {
            self.block_dev.write_block(&slot.block);
            slot.dirty = false;
        }
    }

    /// Flush all blocks to disk.
    pub fn flush_all(&self) {
        let mut cache = self.cache.lock();
        for set in cache.iter_mut() {
            for slot in set.slots.iter_mut() {
                if slot.valid && slot.dirty {
                    trace!("Flushing block {} to disk", slot.block.id);
                    self.block_dev.write_block(&slot.block);
                    slot.dirty = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicUsize, Ordering};

    struct MemoryDevice {
        reads: AtomicUsize,
        bulk_reads: AtomicUsize,
        writes: Mutex<Vec<Block>>,
    }

    impl BlockDevice for MemoryDevice {
        fn read_block(&self, block_id: PBlockId) -> Block {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let mut data = Box::new([0; BLOCK_SIZE]);
            data[0] = block_id as u8;
            Block::new(block_id, data)
        }

        fn read_blocks(&self, start_block: PBlockId, buf: &mut [u8]) {
            self.bulk_reads.fetch_add(1, Ordering::Relaxed);
            for (index, chunk) in buf.chunks_exact_mut(BLOCK_SIZE).enumerate() {
                chunk.fill((start_block + index as PBlockId) as u8);
            }
        }

        fn write_block(&self, block: &Block) {
            self.writes.lock().push(block.clone());
        }
    }

    #[test]
    fn cache_hit_shares_data_and_write_uses_copy_on_write() {
        let device = Arc::new(MemoryDevice {
            reads: AtomicUsize::new(0),
            bulk_reads: AtomicUsize::new(0),
            writes: Mutex::new(Vec::new()),
        });
        let cache = BlockCache::new(device.clone());

        let cached = cache.read_block(7);
        let mut writable = cache.read_block(7);
        assert_eq!(device.reads.load(Ordering::Relaxed), 1);
        assert!(Arc::ptr_eq(&cached.data, &writable.data));

        writable.write_offset(0, &[99]);
        assert_eq!(cached.data()[0], 7);
        assert_eq!(writable.data()[0], 99);
        assert!(!Arc::ptr_eq(&cached.data, &writable.data));

        cache.write_block(&writable);
        cache.flush(7);
        let writes = device.writes.lock();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].data()[0], 99);
    }

    #[test]
    fn contiguous_bulk_read_uses_one_backend_call_and_overlays_dirty_hit() {
        let device = Arc::new(MemoryDevice {
            reads: AtomicUsize::new(0),
            bulk_reads: AtomicUsize::new(0),
            writes: Mutex::new(Vec::new()),
        });
        let cache = BlockCache::new(device.clone());
        let mut dirty = cache.read_block(8);
        dirty.data_mut().fill(99);
        cache.write_block(&dirty);

        let mut data = vec![0u8; BLOCK_SIZE * 3];
        cache.read_blocks(7, &mut data);

        assert_eq!(device.bulk_reads.load(Ordering::Relaxed), 1);
        assert_eq!(data[0], 7);
        assert_eq!(data[BLOCK_SIZE], 99);
        assert_eq!(data[BLOCK_SIZE * 2], 9);
    }
}
