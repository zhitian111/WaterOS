//! Reverse mappings for directly mapped read-only file pages.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use spin::{Mutex, RwLock, RwLockReadGuard};

use crate::error::{MmError, MmResult};
use crate::mmap::FileObjectId;

/// Architecture-owned invalidation action for one resident file PTE.
pub trait RegisteredFileMapping: Send {
    fn page_index(&self) -> usize;
    fn generation(&self) -> u64;
    fn duplicate_for(&self, pte_context : usize) -> MmResult<Box<dyn RegisteredFileMapping>>;
    /// Bind a registration created while constructing an address space to its
    /// stable address-space handle.  Before this point the architecture may
    /// use `pte_context` only because the address space cannot be scheduled.
    fn bind_aspace(&mut self, aspace_handle : usize);
    fn invalidate(self : Box<Self>);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileMappingRegistration {
    pub file_id : FileObjectId,
    pub id : u64,
}

struct Registry {
    files : BTreeMap<FileObjectId, BTreeMap<u64, Box<dyn RegisteredFileMapping>>>,
    inflight : BTreeSet<(FileObjectId, u64)>,
    next_id : u64,
}

impl Registry {
    const fn new() -> Self {
        Self { files : BTreeMap::new(), inflight : BTreeSet::new(), next_id : 1 }
    }
}

const REGISTRY_SHARDS : usize = 64;
static REGISTRIES : [Mutex<Registry>; REGISTRY_SHARDS] =
    [const { Mutex::new(Registry::new()) }; REGISTRY_SHARDS];
/// Serializes VFS-driven PTE invalidation against the fork interval which
/// copies PTEs and then installs the corresponding child reverse mappings.
static INVALIDATION_GATE : RwLock<()> = RwLock::new(());

#[inline]
fn registry(file_id : FileObjectId) -> &'static Mutex<Registry> {
    let mixed = file_id.mount_id ^ file_id.inode_id.rotate_left(23);
    &REGISTRIES[mixed as usize & (REGISTRY_SHARDS - 1)]
}

/// Freeze VFS-driven invalidation while an address-space implementation
/// copies shared file PTEs and duplicates their registry entries.
pub fn freeze_invalidation() -> RwLockReadGuard<'static, ()> {
    INVALIDATION_GATE.read()
}

pub fn register(file_id : FileObjectId,
                mapping : Box<dyn RegisteredFileMapping>)
                -> MmResult<FileMappingRegistration> {
    let mut registry = registry(file_id).lock();
    let id = registry.next_id;
    registry.next_id = registry.next_id.checked_add(1).ok_or(MmError::OutOfMemory)?;
    registry.files.entry(file_id).or_default().insert(id, mapping);
    Ok(FileMappingRegistration { file_id, id })
}

/// Remove a registration while retaining its cache/PTE lifetime token.
/// Callers must keep the returned mapping alive until the corresponding PTE
/// can no longer access the shared frame.
pub fn take(registration : FileMappingRegistration) -> Option<Box<dyn RegisteredFileMapping>> {
    loop {
        let mut registry = registry(registration.file_id).lock();
        let removed = if let Some(file) = registry.files.get_mut(&registration.file_id) {
            let removed = file.remove(&registration.id);
            if file.is_empty() {
                registry.files.remove(&registration.file_id);
            }
            removed
        } else {
            None
        };
        if removed.is_some() {
            return removed;
        }
        if !registry.inflight.contains(&(registration.file_id, registration.id)) {
            return None;
        }
        drop(registry);
        core::hint::spin_loop();
    }
}

pub fn unregister(registration : FileMappingRegistration) -> bool {
    take(registration).is_some()
}

/// Attach a newly published address-space handle to an existing registration.
pub fn bind_aspace(registration : FileMappingRegistration, aspace_handle : usize) -> MmResult<()> {
    let mut registry = registry(registration.file_id).lock();
    let mapping = registry.files
                          .get_mut(&registration.file_id)
                          .and_then(|file| file.get_mut(&registration.id))
                          .ok_or(MmError::NotMapped)?;
    mapping.bind_aspace(aspace_handle);
    Ok(())
}

pub fn duplicate(registration : FileMappingRegistration,
                 pte_context : usize)
                 -> MmResult<FileMappingRegistration> {
    let duplicate = {
        let registry = registry(registration.file_id).lock();
        registry.files
                .get(&registration.file_id)
                .and_then(|file| file.get(&registration.id))
                .ok_or(MmError::NotMapped)?
                .duplicate_for(pte_context)?
    };
    register(registration.file_id, duplicate)
}

fn invalidate_matching(file_id : FileObjectId,
                       mut matches : impl FnMut(&dyn RegisteredFileMapping) -> bool) {
    let _invalidation = INVALIDATION_GATE.write();
    let mappings = {
        let mut registry = registry(file_id).lock();
        let Some(file) = registry.files.get_mut(&file_id) else {
            return;
        };
        let ids : Vec<u64> = file.iter()
                               .filter_map(|(&id, mapping)| matches(mapping.as_ref()).then_some(id))
                               .collect();
        let mut mappings = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(mapping) = file.remove(&id) {
                mappings.push((id, mapping));
            }
        }
        if file.is_empty() {
            registry.files.remove(&file_id);
        }
        for (id, _) in &mappings {
            registry.inflight.insert((file_id, *id));
        }
        mappings
    };
    for (id, mapping) in mappings {
        mapping.invalidate();
        registry(file_id).lock().inflight.remove(&(file_id, id));
    }
}

/// Prepare a byte-range write by revoking every overlapping resident page.
pub fn prepare_write(file_id : FileObjectId, start : u64, len : usize) {
    if len == 0 {
        return;
    }
    let first = start / crate::addr::PAGE_SIZE as u64;
    let last = start.saturating_add(len.saturating_sub(1) as u64) /
               crate::addr::PAGE_SIZE as u64;
    invalidate_matching(file_id, |mapping| {
        let page = mapping.page_index() as u64;
        page >= first && page <= last
    });
}

/// Revoke the truncated tail page and every page beyond the new EOF.
pub fn truncate(file_id : FileObjectId, new_size : u64) {
    let first = new_size / crate::addr::PAGE_SIZE as u64;
    invalidate_matching(file_id, |mapping| mapping.page_index() as u64 >= first);
}

pub fn invalidate_all(file_id : FileObjectId, generation : u64) {
    invalidate_matching(file_id, |mapping| mapping.generation() == generation);
}

#[cfg(test)]
mod tests {
    use alloc::boxed::Box;
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct TestMapping {
        page : usize,
        generation : u64,
        invalidated : Arc<AtomicUsize>,
    }

    impl RegisteredFileMapping for TestMapping {
        fn page_index(&self) -> usize { self.page }
        fn generation(&self) -> u64 { self.generation }
        fn duplicate_for(&self, _pte_context : usize) -> MmResult<Box<dyn RegisteredFileMapping>> {
            Ok(Box::new(Self { page : self.page,
                               generation : self.generation,
                               invalidated : self.invalidated.clone() }))
        }
        fn bind_aspace(&mut self, _aspace_handle : usize) {}
        fn invalidate(self : Box<Self>) {
            self.invalidated.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn write_range_invalidates_only_overlapping_pages() {
        let file = FileObjectId { mount_id : 0xfeed, inode_id : 1 };
        let count = Arc::new(AtomicUsize::new(0));
        let first = register(file, Box::new(TestMapping { page : 1,
                                                          generation : 7,
                                                          invalidated : count.clone() })).unwrap();
        let second = register(file, Box::new(TestMapping { page : 3,
                                                           generation : 7,
                                                           invalidated : count.clone() })).unwrap();
        prepare_write(file, crate::addr::PAGE_SIZE as u64, 8);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(!unregister(first));
        assert!(unregister(second));
    }

    #[test]
    fn truncate_revokes_tail_page_and_fork_duplicate() {
        let file = FileObjectId { mount_id : 0xfeed, inode_id : 2 };
        let count = Arc::new(AtomicUsize::new(0));
        let parent = register(file, Box::new(TestMapping { page : 2,
                                                           generation : 9,
                                                           invalidated : count.clone() })).unwrap();
        let child = duplicate(parent, 0).unwrap();
        truncate(file, 2 * crate::addr::PAGE_SIZE as u64 + 1);
        assert_eq!(count.load(Ordering::Relaxed), 2);
        assert!(!unregister(parent));
        assert!(!unregister(child));
    }
}
