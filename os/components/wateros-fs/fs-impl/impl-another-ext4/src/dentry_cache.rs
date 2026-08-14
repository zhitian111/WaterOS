//! another-ext4 路径查找的负 dentry 缓存。
//!
//! 该缓存只负责路径哈希、固定容量淘汰和子树失效；磁盘 inode 查找与 FS 生命周期
//! 由父模块负责，避免把缓存策略和 ext4 I/O 耦合在同一文件中。

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const CAPACITY : usize = 4096;
const WAYS : usize = 4;
const BUCKETS : usize = CAPACITY / WAYS;
const FNV1A_OFFSET : u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME : u64 = 0x0000_0100_0000_01b3;

pub(crate) fn negative_path_hash(path : &str) -> u64 {
    path.as_bytes()
        .iter()
        .fold(FNV1A_OFFSET, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV1A_PRIME)
        })
}

struct NegativeDentry {
    hash : u64,
    path : String,
}

pub(crate) struct NegativeDentryCache {
    slots : Vec<Option<NegativeDentry>>,
    next_victim : Vec<u8>,
}

impl NegativeDentryCache {
    pub(crate) fn new() -> Self {
        let mut slots = Vec::with_capacity(CAPACITY);
        slots.resize_with(CAPACITY, || None);
        Self { slots,
               next_victim : vec![0; BUCKETS] }
    }

    pub(crate) fn bucket(hash : u64) -> usize { hash as usize % BUCKETS }

    pub(crate) fn contains(&self, path : &str) -> bool {
        let hash = negative_path_hash(path);
        let first = Self::bucket(hash) * WAYS;
        self.slots[first..first + WAYS]
            .iter()
            .flatten()
            .any(|entry| entry.hash == hash && entry.path == path)
    }

    pub(crate) fn insert(&mut self, path : &str) {
        let hash = negative_path_hash(path);
        let bucket = Self::bucket(hash);
        let first = bucket * WAYS;
        let ways = &mut self.slots[first..first + WAYS];
        if ways.iter()
               .flatten()
               .any(|entry| entry.hash == hash && entry.path == path)
        {
            return;
        }
        let way = ways.iter()
                      .position(Option::is_none)
                      .unwrap_or_else(|| {
                          let way = usize::from(self.next_victim[bucket]);
                          self.next_victim[bucket] = ((way + 1) % WAYS) as u8;
                          way
                      });
        ways[way] = Some(NegativeDentry { hash,
                                          path : String::from(path) });
    }

    pub(crate) fn remove_exact(&mut self, path : &str) -> usize {
        let hash = negative_path_hash(path);
        let first = Self::bucket(hash) * WAYS;
        for slot in self.slots[first..first + WAYS].iter_mut() {
            if slot.as_ref()
                   .is_some_and(|entry| entry.hash == hash && entry.path == path)
            {
                *slot = None;
                return 1;
            }
        }
        0
    }

    pub(crate) fn remove_subtree(&mut self, path : &str) -> usize {
        let prefix = if path.ends_with('/') {
            String::from(path)
        } else {
            let mut prefix = String::from(path);
            prefix.push('/');
            prefix
        };
        let mut removed = 0usize;
        for slot in self.slots.iter_mut() {
            let matches = slot.as_ref()
                              .is_some_and(|entry| {
                                  entry.path == path || entry.path.starts_with(prefix.as_str())
                              });
            if matches {
                *slot = None;
                removed += 1;
            }
        }
        removed
    }
}
