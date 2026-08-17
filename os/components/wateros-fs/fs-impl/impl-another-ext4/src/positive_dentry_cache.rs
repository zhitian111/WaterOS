//! another-ext4 正 dentry 的固定容量 second-chance cache。

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

struct PositiveDentry {
    inode : u32,
    slot : usize,
    referenced : bool,
}

pub(crate) struct PositiveDentryCache {
    entries : BTreeMap<String, PositiveDentry>,
    slots : Vec<Option<String>>,
    hand : usize,
}

impl PositiveDentryCache {
    pub(crate) const fn new() -> Self {
        Self { entries : BTreeMap::new(),
               slots : Vec::new(),
               hand : 0 }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize { self.entries.len() }

    pub(crate) fn get(&mut self, path : &str) -> Option<u32> {
        let entry = self.entries.get_mut(path)?;
        entry.referenced = true;
        Some(entry.inode)
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, path : &str) -> bool { self.entries.contains_key(path) }

    pub(crate) fn insert(&mut self, path : &str, inode : u32, capacity : usize) -> usize {
        if let Some(entry) = self.entries.get_mut(path) {
            entry.inode = inode;
            entry.referenced = true;
            return 0;
        }
        if capacity == 0 {
            return 0;
        }

        let (slot, evicted) = if self.entries.len() < capacity {
            (self.vacant_slot(capacity), 0)
        } else {
            (self.clock_victim(capacity), 1)
        };
        let owned_path = String::from(path);
        self.slots[slot] = Some(owned_path.clone());
        self.entries.insert(owned_path,
                            PositiveDentry { inode,
                                             slot,
                                             referenced : true });
        evicted
    }

    fn vacant_slot(&mut self, capacity : usize) -> usize {
        if self.slots.len() < capacity {
            let slot = self.slots.len();
            self.slots.push(None);
            return slot;
        }
        self.slots.iter()
                  .position(Option::is_none)
                  .expect("positive dentry cache length requires a vacant slot")
    }

    fn clock_victim(&mut self, capacity : usize) -> usize {
        loop {
            let slot = self.hand;
            self.hand = (self.hand + 1) % capacity;
            let path = self.slots[slot]
                           .as_ref()
                           .expect("full positive dentry cache has no vacant slots")
                           .clone();
            let entry = self.entries
                            .get_mut(path.as_str())
                            .expect("positive dentry slot and index must agree");
            if entry.referenced {
                entry.referenced = false;
                continue;
            }
            self.entries.remove(path.as_str());
            self.slots[slot] = None;
            return slot;
        }
    }

    fn remove_exact(&mut self, path : &str) -> bool {
        let Some(entry) = self.entries.remove(path) else {
            return false;
        };
        self.slots[entry.slot] = None;
        true
    }

    pub(crate) fn remove_subtree(&mut self, path : &str) -> usize {
        let prefix = subtree_prefix(path);
        let removed : Vec<String> = self.entries
                                        .keys()
                                        .filter(|cached| {
                                            cached.as_str() == path ||
                                            cached.starts_with(prefix.as_str())
                                        })
                                        .cloned()
                                        .collect();
        for cached in removed.iter() {
            self.remove_exact(cached.as_str());
        }
        removed.len()
    }

    pub(crate) fn rename_subtree(&mut self,
                                  old_path : &str,
                                  new_path : &str,
                                  capacity : usize)
                                  -> usize {
        let old_prefix = subtree_prefix(old_path);
        let moved : Vec<(String, u32)> = self.entries
                                              .iter()
                                              .filter_map(|(cached, entry)| {
                                                  if cached == old_path {
                                                      return Some((String::from(new_path),
                                                                   entry.inode));
                                                  }
                                                  cached.strip_prefix(old_prefix.as_str())
                                                        .map(|suffix| {
                                                            let mut renamed =
                                                                subtree_prefix(new_path);
                                                            renamed.push_str(suffix);
                                                            (renamed, entry.inode)
                                                        })
                                              })
                                              .collect();
        self.remove_subtree(old_path);
        self.remove_subtree(new_path);
        moved.into_iter()
             .map(|(path, inode)| self.insert(path.as_str(), inode, capacity))
             .sum()
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.slots.clear();
        self.hand = 0;
    }
}

fn subtree_prefix(path : &str) -> String {
    if path.ends_with('/') {
        String::from(path)
    } else {
        let mut prefix = String::from(path);
        prefix.push('/');
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::PositiveDentryCache;

    #[test]
    fn capacity_pressure_evicts_one_entry() {
        let mut cache = PositiveDentryCache::new();
        for index in 0..4 {
            cache.insert(alloc::format!("/{index}").as_str(), index, 4);
        }
        assert_eq!(cache.insert("/4", 4, 4), 1);
        assert_eq!(cache.len(), 4);
        assert!(!cache.contains("/0"));
        assert!(cache.contains("/4"));
    }

    #[test]
    fn production_capacity_remains_bounded_without_bulk_clear() {
        let mut cache = PositiveDentryCache::new();
        for index in 0..=4096 {
            cache.insert(alloc::format!("/entry/{index}").as_str(), index, 4096);
        }
        assert_eq!(cache.len(), 4096);
        assert!(!cache.contains("/entry/0"));
        assert!(cache.contains("/entry/1"));
        assert!(cache.contains("/entry/4096"));
    }

    #[test]
    fn recently_used_entries_receive_a_second_chance() {
        let mut cache = PositiveDentryCache::new();
        for index in 0..8 {
            cache.insert(alloc::format!("/{index}").as_str(), index, 8);
        }
        cache.insert("/8", 8, 8);
        assert_eq!(cache.get("/1"), Some(1));
        cache.insert("/9", 9, 8);
        assert!(cache.contains("/1"));
        assert!(!cache.contains("/2"));
    }

    #[test]
    fn subtree_remove_and_rename_keep_index_and_slots_consistent() {
        let mut cache = PositiveDentryCache::new();
        cache.insert("/src", 1, 8);
        cache.insert("/src/child", 2, 8);
        cache.insert("/dst/stale", 3, 8);
        cache.insert("/other", 4, 8);

        assert_eq!(cache.rename_subtree("/src", "/dst", 8), 0);
        assert_eq!(cache.get("/dst"), Some(1));
        assert_eq!(cache.get("/dst/child"), Some(2));
        assert!(!cache.contains("/dst/stale"));
        assert_eq!(cache.remove_subtree("/dst"), 2);
        assert_eq!(cache.get("/other"), Some(4));
    }
}
