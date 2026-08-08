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
