//! 各 arch `mm-impl` 共享的实现辅助逻辑（ELF 装载、mmap/mremap、按需零页等）。
//!
//! 本 crate **不**对外暴露稳定契约，位于 `wateros-mm-api-v0` 之下：可依赖当前
//! loader 策略与 bring-up 假设；语义边界仍以 `mm-api` 为准。

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use core::cmp;
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::executable;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::kernel_bringup::LoadElfError;
use api_v0::mmap::DemandPageLoader;
use api_v0::perm::PagePerm;
use core::sync::atomic::AtomicU64;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result, frame_inc_ref, frame_ref_count};
use vfs_api::VfsFileContentIdentity;

/// 私有匿名映射的惰性缺页 loader：缺页时不做任何加载，
/// 直接保留 `handle_lazy_page_fault` 预先清零的页（等价于按需零页）。
///
/// 复用文件 lazy VMA 机制，避免匿名 mmap 饥渴分配整段物理帧
/// （例如 glibc pthread 每线程 8 MiB 栈，批量创建会瞬间耗尽帧池 → `ENOMEM`）。
pub struct ZeroAnonLoader;

impl DemandPageLoader for ZeroAnonLoader {
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>> { Ok(Box::new(ZeroAnonLoader)) }

    fn load_page(&mut self, _file_offset : usize, _dst : &mut [u8]) -> MmResult<()> { Ok(()) }
}

/// PT_LOAD 惰性缺页：按页从 ELF 文件区间填充 `dst`（段前/BSS 由调用方预先清零）。
pub fn fill_elf_load_page<F>(vbase : usize,
                             p_offset : usize,
                             filesz : usize,
                             page_va : usize,
                             dst : &mut [u8],
                             mut read_file : F)
                             -> MmResult<()>
    where F : FnMut(usize, &mut [u8]) -> MmResult<()>
{
    let page_end = page_va.checked_add(dst.len())
                          .ok_or(MmError::InvalidAddress)?;
    let file_end_va = vbase.checked_add(filesz)
                           .ok_or(MmError::InvalidAddress)?;
    let seg_start = cmp::max(page_va, vbase);
    let seg_end = cmp::min(page_end, file_end_va);
    if seg_start >= seg_end {
        return Ok(());
    }
    let dst_off = seg_start - page_va;
    let rel = seg_start.checked_sub(vbase)
                       .ok_or(MmError::InvalidAddress)?;
    let len = seg_end - seg_start;
    let file_pos = p_offset.checked_add(rel)
                           .ok_or(MmError::InvalidAddress)?;
    read_file(file_pos, &mut dst[dst_off..dst_off + len])
}

/// execve lazy map 登记 VMA 时的段参数（供各 arch `kernel_elf` 构造 loader）。
#[derive(Clone, Debug)]
pub struct ElfSegmentLoadParams {
    pub vbase : usize,
    pub p_offset : usize,
    pub filesz : usize,
    pub vma_start : usize,
    pub vma_file_origin : usize,
}

const ELF_READONLY_PAGE_CACHE_CAPACITY : usize = 16_384;
#[cfg(feature = "cache-layer-diagnostics")]
const ELF_CACHE_DIAGNOSTIC_REPORT_LOOKUPS : u64 = 1 << 14;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ElfReadonlyPageKey {
    mount_generation : u64,
    mount_id : u64,
    node_id : u64,
    content_version : u64,
    vbase : usize,
    p_offset : usize,
    filesz : usize,
    vma_start : usize,
    file_offset : usize,
}

struct ElfReadonlyPageEntry {
    ppn : PhysPageNum,
    last_used : u64,
    _identity : VfsFileContentIdentity,
}

struct ElfReadonlyPageCache {
    entries : BTreeMap<ElfReadonlyPageKey, ElfReadonlyPageEntry>,
    tick : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    hits : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    misses : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    installs : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    duplicate_loads : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    evictions : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    next_report : u64,
}

impl ElfReadonlyPageCache {
    fn new() -> Self {
        Self { entries : BTreeMap::new(),
               tick : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               hits : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               misses : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               installs : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               duplicate_loads : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               evictions : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               next_report : ELF_CACHE_DIAGNOSTIC_REPORT_LOOKUPS }
    }

    #[cfg(feature = "cache-layer-diagnostics")]
    fn note_lookup(&mut self, hit : bool) {
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        let total = self.hits + self.misses;
        if total < self.next_report {
            return;
        }
        self.next_report = total.saturating_add(ELF_CACHE_DIAGNOSTIC_REPORT_LOOKUPS);
        runtime::logging::error!("[cache-diag:elf] lookups={} hit={} miss={} installs={} \
                                  duplicate_load={} evict={} resident={}",
                                 total,
                                 self.hits,
                                 self.misses,
                                 self.installs,
                                 self.duplicate_loads,
                                 self.evictions,
                                 self.entries.len());
    }
}

static ELF_READONLY_PAGE_CACHE : spin::Mutex<Option<ElfReadonlyPageCache>> =
    spin::Mutex::new(None);

fn readonly_page_key(identity : &VfsFileContentIdentity,
                     content_version : u64,
                     params : &ElfSegmentLoadParams,
                     file_offset : usize)
                     -> ElfReadonlyPageKey {
    ElfReadonlyPageKey { mount_generation : identity.mount_generation(),
                         mount_id : identity.mount_id(),
                         node_id : identity.node_id(),
                         content_version,
                         vbase : params.vbase,
                         p_offset : params.p_offset,
                         filesz : params.filesz,
                         vma_start : params.vma_start,
                         file_offset }
}

/// Load or reuse one immutable ELF page. The returned frame owns one reference
/// for the caller's future mapping; the cache retains a separate reference.
/// The loader runs without the cache lock, so concurrent misses may perform
/// duplicate I/O but publish only one frame.
pub fn load_or_get_readonly_elf_page<F>(identity : &VfsFileContentIdentity,
                                        params : &ElfSegmentLoadParams,
                                        file_offset : usize,
                                        mut load : F)
                                        -> MmResult<PhysPageNum>
    where F : FnMut(&mut [u8]) -> MmResult<()>
{
    loop {
        let content_version = identity.version();
        let key = readonly_page_key(identity, content_version, params, file_offset);
        let cached = {
            let mut guard = ELF_READONLY_PAGE_CACHE.lock();
            let cache = guard.get_or_insert_with(ElfReadonlyPageCache::new);
            cache.tick = cache.tick.wrapping_add(1);
            let tick = cache.tick;
            #[cfg(feature = "cache-layer-diagnostics")]
            {
                let hit = cache.entries.contains_key(&key);
                cache.note_lookup(hit);
            }
            if let Some(entry) = cache.entries.get_mut(&key) {
                frame_inc_ref(entry.ppn).map_err(MmError::from)?;
                entry.last_used = tick;
                Some(entry.ppn)
            } else {
                None
            }
        };
        if let Some(ppn) = cached {
            if identity.version() == content_version {
                return Ok(ppn);
            }
            let _ = frame_dealloc_result(ppn);
            continue;
        }

        let loaded_ppn = frame_alloc_result().map_err(MmError::from)?;
        let pa = loaded_ppn.0 * PAGE_SIZE;
        let dst = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        dst.fill(0);
        if let Err(error) = load(dst) {
            let _ = frame_dealloc_result(loaded_ppn);
            return Err(error);
        }
        if identity.version() != content_version {
            let _ = frame_dealloc_result(loaded_ppn);
            continue;
        }

        let mut duplicate = None;
        let mut evicted = None;
        {
            let mut guard = ELF_READONLY_PAGE_CACHE.lock();
            let cache = guard.get_or_insert_with(ElfReadonlyPageCache::new);
            cache.tick = cache.tick.wrapping_add(1);
            let tick = cache.tick;
            if let Some(entry) = cache.entries.get_mut(&key) {
                frame_inc_ref(entry.ppn).map_err(MmError::from)?;
                entry.last_used = tick;
                duplicate = Some(entry.ppn);
                #[cfg(feature = "cache-layer-diagnostics")]
                {
                    cache.duplicate_loads += 1;
                }
            } else {
                if cache.entries.len() >= ELF_READONLY_PAGE_CACHE_CAPACITY {
                    let victim = cache.entries
                                      .iter()
                                      .min_by_key(|(_, entry)| entry.last_used)
                                      .map(|(key, _)| *key);
                    if let Some(victim) = victim {
                        evicted = cache.entries.remove(&victim).map(|entry| entry.ppn);
                        #[cfg(feature = "cache-layer-diagnostics")]
                        if evicted.is_some() {
                            cache.evictions += 1;
                        }
                    }
                }
                cache.entries.insert(key,
                                     ElfReadonlyPageEntry { ppn : loaded_ppn,
                                                            last_used : tick,
                                                            _identity : identity.clone() });
                frame_inc_ref(loaded_ppn).map_err(MmError::from)?;
                #[cfg(feature = "cache-layer-diagnostics")]
                {
                    cache.installs += 1;
                }
            }
        }
        if let Some(ppn) = evicted {
            let _ = frame_dealloc_result(ppn);
        }
        let result_ppn = if let Some(ppn) = duplicate {
            let _ = frame_dealloc_result(loaded_ppn);
            ppn
        } else {
            loaded_ppn
        };
        if identity.version() != content_version {
            let _ = frame_dealloc_result(result_ppn);
            continue;
        }
        return Ok(result_ppn);
    }
}

/// Directed cache/refcount self-test; callers must initialize the global frame
/// allocator first.
pub fn test_readonly_elf_page_cache() {
    let identity = VfsFileContentIdentity::new(usize::MAX as u64,
                                               u64::MAX - 1,
                                               u64::MAX,
                                               Arc::new(AtomicU64::new(1)));
    let params = ElfSegmentLoadParams { vbase : 0x10_0000,
                                        p_offset : 0,
                                        filesz : PAGE_SIZE,
                                        vma_start : 0x10_0000,
                                        vma_file_origin : 0 };
    let mut loads = 0usize;
    let first = load_or_get_readonly_elf_page(&identity, &params, 0, |dst| {
                    loads += 1;
                    dst[0] = 0x5a;
                    Ok(())
                }).expect("first readonly ELF cache load");
    let second = load_or_get_readonly_elf_page(&identity, &params, 0, |dst| {
                     loads += 1;
                     dst[0] = 0xa5;
                     Ok(())
                 }).expect("readonly ELF cache hit");
    assert_eq!(first, second);
    assert_eq!(loads, 1, "same content key must load only once");
    assert_eq!(frame_ref_count(first).expect("cached frame refcount"),
               3,
               "cache plus two mapping references");
    frame_dealloc_result(first).expect("release first mapping ref");
    frame_dealloc_result(second).expect("release second mapping ref");

    identity.mark_changed();
    let changed = load_or_get_readonly_elf_page(&identity, &params, 0, |dst| {
                      loads += 1;
                      dst[0] = 0xa5;
                      Ok(())
                  }).expect("changed content version cache miss");
    assert_ne!(changed, first);
    assert_eq!(loads, 2, "content version change must reload the page");
    frame_dealloc_result(changed).expect("release changed mapping ref");
}

const MMAP_READONLY_PAGE_CACHE_CAPACITY : usize = 49_152;
#[cfg(feature = "cache-layer-diagnostics")]
const MMAP_CACHE_DIAGNOSTIC_REPORT_LOOKUPS : u64 = 1 << 14;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MmapReadonlyPageKey {
    mount_generation : u64,
    mount_id : u64,
    node_id : u64,
    content_version : u64,
    file_offset : usize,
    mapping_file_size : usize,
}

struct MmapReadonlyPageEntry {
    ppn : PhysPageNum,
    last_used : u64,
    _identity : VfsFileContentIdentity,
}

struct MmapReadonlyPageCache {
    entries : BTreeMap<MmapReadonlyPageKey, MmapReadonlyPageEntry>,
    tick : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    hits : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    misses : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    installs : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    duplicate_loads : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    full_bypasses : u64,
    #[cfg(feature = "cache-layer-diagnostics")]
    next_report : u64,
}

impl MmapReadonlyPageCache {
    fn new() -> Self {
        Self { entries : BTreeMap::new(),
               tick : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               hits : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               misses : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               installs : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               duplicate_loads : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               full_bypasses : 0,
               #[cfg(feature = "cache-layer-diagnostics")]
               next_report : MMAP_CACHE_DIAGNOSTIC_REPORT_LOOKUPS }
    }

    #[cfg(feature = "cache-layer-diagnostics")]
    fn note_lookup(&mut self, hit : bool) {
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        let total = self.hits + self.misses;
        if total < self.next_report {
            return;
        }
        self.next_report = total.saturating_add(MMAP_CACHE_DIAGNOSTIC_REPORT_LOOKUPS);
        runtime::logging::error!("[cache-diag:mmap-ro] lookups={} hit={} miss={} installs={} \
                                  duplicate_load={} full_bypass={} resident={}",
                                 total,
                                 self.hits,
                                 self.misses,
                                 self.installs,
                                 self.duplicate_loads,
                                 self.full_bypasses,
                                 self.entries.len());
    }
}

static MMAP_READONLY_PAGE_CACHE : spin::Mutex<Option<MmapReadonlyPageCache>> =
    spin::Mutex::new(None);

fn mmap_readonly_page_key(identity : &VfsFileContentIdentity,
                          content_version : u64,
                          file_offset : usize,
                          mapping_file_size : usize)
                          -> MmapReadonlyPageKey {
    MmapReadonlyPageKey { mount_generation : identity.mount_generation(),
                          mount_id : identity.mount_id(),
                          node_id : identity.node_id(),
                          content_version,
                          file_offset,
                          mapping_file_size }
}

/// Load or reuse one immutable page from a private file mapping. The cache and
/// the returned mapping each own one frame reference. File I/O runs outside the
/// cache lock; a content-version change retries under a fresh key.
pub fn load_or_get_readonly_mmap_page<F>(identity : &VfsFileContentIdentity,
                                         file_offset : usize,
                                         mapping_file_size : usize,
                                         mut load : F)
                                         -> MmResult<PhysPageNum>
    where F : FnMut(&mut [u8]) -> MmResult<()>
{
    debug_assert_eq!(file_offset % PAGE_SIZE, 0);
    loop {
        let content_version = identity.version();
        let key = mmap_readonly_page_key(identity,
                                         content_version,
                                         file_offset,
                                         mapping_file_size);
        let cached = {
            let mut guard = MMAP_READONLY_PAGE_CACHE.lock();
            let cache = guard.get_or_insert_with(MmapReadonlyPageCache::new);
            cache.tick = cache.tick.wrapping_add(1);
            let tick = cache.tick;
            #[cfg(feature = "cache-layer-diagnostics")]
            {
                let hit = cache.entries.contains_key(&key);
                cache.note_lookup(hit);
            }
            if let Some(entry) = cache.entries.get_mut(&key) {
                frame_inc_ref(entry.ppn).map_err(MmError::from)?;
                entry.last_used = tick;
                Some(entry.ppn)
            } else {
                None
            }
        };
        if let Some(ppn) = cached {
            if identity.version() == content_version {
                return Ok(ppn);
            }
            let _ = frame_dealloc_result(ppn);
            continue;
        }

        let loaded_ppn = frame_alloc_result().map_err(MmError::from)?;
        let pa = loaded_ppn.0 * PAGE_SIZE;
        let dst = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        dst.fill(0);
        if let Err(error) = load(dst) {
            let _ = frame_dealloc_result(loaded_ppn);
            return Err(error);
        }
        if identity.version() != content_version {
            let _ = frame_dealloc_result(loaded_ppn);
            continue;
        }

        let mut duplicate = None;
        {
            let mut guard = MMAP_READONLY_PAGE_CACHE.lock();
            let cache = guard.get_or_insert_with(MmapReadonlyPageCache::new);
            cache.tick = cache.tick.wrapping_add(1);
            let tick = cache.tick;
            if let Some(entry) = cache.entries.get_mut(&key) {
                frame_inc_ref(entry.ppn).map_err(MmError::from)?;
                entry.last_used = tick;
                duplicate = Some(entry.ppn);
                #[cfg(feature = "cache-layer-diagnostics")]
                {
                    cache.duplicate_loads += 1;
                }
            } else {
                if cache.entries.len() >= MMAP_READONLY_PAGE_CACHE_CAPACITY {
                    // Keep the established hot set and return the freshly loaded
                    // frame only to this mapping. Generic file mmap streams can
                    // contain millions of one-shot pages; an O(n) LRU victim scan
                    // on every miss is worse than bypassing admission.
                    #[cfg(feature = "cache-layer-diagnostics")]
                    {
                        cache.full_bypasses += 1;
                    }
                } else {
                    cache.entries.insert(key,
                                         MmapReadonlyPageEntry { ppn : loaded_ppn,
                                                                 last_used : tick,
                                                                 _identity : identity.clone() });
                    frame_inc_ref(loaded_ppn).map_err(MmError::from)?;
                    #[cfg(feature = "cache-layer-diagnostics")]
                    {
                        cache.installs += 1;
                    }
                }
            }
        }
        let result_ppn = if let Some(ppn) = duplicate {
            let _ = frame_dealloc_result(loaded_ppn);
            ppn
        } else {
            loaded_ppn
        };
        if identity.version() != content_version {
            let _ = frame_dealloc_result(result_ppn);
            continue;
        }
        return Ok(result_ppn);
    }
}

/// Directed generic mmap cache/refcount self-test; callers initialize frames.
pub fn test_readonly_mmap_page_cache() {
    let identity = VfsFileContentIdentity::new(usize::MAX as u64 - 1,
                                               u64::MAX - 2,
                                               u64::MAX - 1,
                                               Arc::new(AtomicU64::new(1)));
    let mut loads = 0usize;
    let first = load_or_get_readonly_mmap_page(&identity, 0, PAGE_SIZE * 2, |dst| {
                    loads += 1;
                    dst[0] = 0x31;
                    Ok(())
                }).expect("first readonly mmap cache load");
    let second = load_or_get_readonly_mmap_page(&identity, 0, PAGE_SIZE * 2, |dst| {
                     loads += 1;
                     dst[0] = 0x32;
                     Ok(())
                 }).expect("readonly mmap cache hit");
    let next_page = load_or_get_readonly_mmap_page(&identity,
                                                   PAGE_SIZE,
                                                   PAGE_SIZE * 2,
                                                   |dst| {
                        loads += 1;
                        dst[0] = 0x33;
                        Ok(())
                    }).expect("different mmap offset cache miss");
    let different_size = load_or_get_readonly_mmap_page(&identity,
                                                        0,
                                                        PAGE_SIZE * 3,
                                                        |dst| {
                             loads += 1;
                             dst[0] = 0x35;
                             Ok(())
                         }).expect("different mmap file-size snapshot cache miss");
    assert_eq!(first, second);
    assert_ne!(first, next_page);
    assert_ne!(first, different_size);
    assert_eq!(loads,
               3,
               "same page must load once while offset and file-size snapshots differ");
    assert_eq!(frame_ref_count(first).expect("cached mmap frame refcount"),
               3,
               "cache plus two mapping references");
    frame_dealloc_result(first).expect("release first mmap mapping ref");
    frame_dealloc_result(second).expect("release second mmap mapping ref");
    frame_dealloc_result(next_page).expect("release next mmap mapping ref");
    frame_dealloc_result(different_size).expect("release different-size mmap mapping ref");

    identity.mark_changed();
    let changed = load_or_get_readonly_mmap_page(&identity, 0, PAGE_SIZE * 2, |dst| {
                      loads += 1;
                      dst[0] = 0x34;
                      Ok(())
                  }).expect("changed mmap content version cache miss");
    assert_ne!(changed, first);
    assert_eq!(loads, 4, "content change must reload a private mmap page");
    frame_dealloc_result(changed).expect("release changed mmap mapping ref");
}

impl ElfSegmentLoadParams {
    pub fn page_va_from_file_offset(&self, file_offset : usize) -> usize {
        self.vma_start + file_offset.saturating_sub(self.vma_file_origin)
    }

    pub fn fill_page<F>(&self,
                        file_offset : usize,
                        dst : &mut [u8],
                        read_file : F)
                        -> MmResult<()>
        where F : FnMut(usize, &mut [u8]) -> MmResult<()>
    {
        fill_elf_load_page(self.vbase,
                           self.p_offset,
                           self.filesz,
                           self.page_va_from_file_offset(file_offset),
                           dst,
                           read_file)
    }
}

/// `PT_LOAD` 程序头类型（可装载段）。
pub const PT_LOAD : u32 = 1;

/// Little-endian `u16` read; returns `None` on out-of-bounds input.
#[inline]
pub fn rd_u16(s : &[u8], o : usize) -> Option<u16> {
    s.get(o..o + 2)?
     .try_into()
     .ok()
     .map(u16::from_le_bytes)
}

/// Little-endian `u32` read; returns `None` on out-of-bounds input.
#[inline]
pub fn rd_u32(s : &[u8], o : usize) -> Option<u32> {
    s.get(o..o + 4)?
     .try_into()
     .ok()
     .map(u32::from_le_bytes)
}

/// Little-endian `u64` read; returns `None` on out-of-bounds input.
#[inline]
pub fn rd_u64(s : &[u8], o : usize) -> Option<u64> {
    s.get(o..o + 8)?
     .try_into()
     .ok()
     .map(u64::from_le_bytes)
}

/// Checks only the ELF64 little-endian prefix accepted by `mm-api`.
#[inline]
pub fn elf64_le_prefix_ok(data : &[u8]) -> bool { executable::is_elf_prefix(data) }

/// Text/script inputs should not trigger ELF read retries.
#[inline]
pub fn skip_elf_prefix_retry(data : &[u8]) -> bool { executable::is_text_file(data) }

/// Checks that `e_entry` is inside a loadable segment.
///
/// This catches images whose ELF prefix looks fine but whose program headers or
/// entry point were read inconsistently from the backing filesystem.
pub fn elf_entry_plausible(data : &[u8]) -> bool {
    if data.len() < 0x40 {
        return false;
    }
    let e_entry = match rd_u64(data, 0x18) {
        Some(v) => v as usize,
        None => return false,
    };
    if e_entry == 0 {
        return false;
    }
    let e_phoff = match rd_u64(data, 0x20) {
        Some(v) => v as usize,
        None => return false,
    };
    let e_phentsize = match rd_u16(data, 0x36) {
        Some(v) => v as usize,
        None => return false,
    };
    let e_phnum = match rd_u16(data, 0x38) {
        Some(v) => v as usize,
        None => return false,
    };
    if e_phentsize < 56 || e_phnum == 0 {
        return false;
    }
    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + 56 > data.len() {
            return false;
        }
        if rd_u32(data, ph) != Some(PT_LOAD) {
            continue;
        }
        let p_vaddr = match rd_u64(data, ph + 16) {
            Some(v) => v as usize,
            None => return false,
        };
        let p_memsz = match rd_u64(data, ph + 40) {
            Some(v) => v as usize,
            None => return false,
        };
        if p_memsz == 0 {
            continue;
        }
        let Some(p_end) = p_vaddr.checked_add(p_memsz) else {
            return false;
        };
        if e_entry >= p_vaddr && e_entry < p_end {
            return true;
        }
    }
    false
}

/// Returns whether an ELF read is acceptable for loading.
#[inline]
pub fn elf_read_acceptable(data : &[u8]) -> bool {
    elf64_le_prefix_ok(data) && elf_entry_plausible(data)
}

/// Stabilizes reads of ELF bytes from a root filesystem.
///
/// If two reads disagree, a third read is used as a tiebreaker; otherwise the
/// first acceptable image is selected. Non-ELF text files are returned as-is so
/// script/shebang probing does not produce noisy retries.
pub fn finalize_elf_read(path : &str,
                         first : Vec<u8>,
                         read_again : impl Fn() -> Result<Vec<u8>, LoadElfError>)
                         -> Result<Vec<u8>, LoadElfError> {
    if skip_elf_prefix_retry(&first) || !elf64_le_prefix_ok(&first) {
        return Ok(first);
    }
    let second = read_again()?;
    if first == second {
        if elf_read_acceptable(&first) {
            return Ok(first);
        }
        if !elf_read_acceptable(&second) {
            let n = second.len().min(16);
            runtime::logging::warn!("[elf-load] stable read bad ELF64-LE image (len={} \
                                     first{}={:02x?}) path={}",
                                    second.len(),
                                    n,
                                    &second[..n],
                                    path);
        }
        return Ok(second);
    }
    runtime::logging::warn!("[elf-load] inconsistent ELF reads path={} len {} vs {}; third read",
                            path,
                            first.len(),
                            second.len());
    let third = read_again()?;
    if second == third && elf_read_acceptable(&second) {
        return Ok(second);
    }
    if first == third && elf_read_acceptable(&first) {
        return Ok(first);
    }
    if elf_read_acceptable(&second) {
        return Ok(second);
    }
    if elf_read_acceptable(&third) {
        return Ok(third);
    }
    if elf_read_acceptable(&first) {
        return Ok(first);
    }
    Ok(second)
}

/// Finds the file offset backing an entry PC inside a `PT_LOAD` segment.
pub fn entry_file_offset(data : &[u8], entry_pc : usize) -> Option<usize> {
    let e_phoff = rd_u64(data, 0x20)? as usize;
    let e_phentsize = rd_u16(data, 0x36)? as usize;
    let e_phnum = rd_u16(data, 0x38)? as usize;
    if e_phentsize < 56 || e_phnum == 0 {
        return None;
    }
    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + 56 > data.len() {
            return None;
        }
        if rd_u32(data, ph)? != PT_LOAD {
            continue;
        }
        let p_vaddr = rd_u64(data, ph + 16)? as usize;
        let p_offset = rd_u64(data, ph + 8)? as usize;
        let p_memsz = rd_u64(data, ph + 40)? as usize;
        let p_end = p_vaddr.checked_add(p_memsz)?;
        if entry_pc >= p_vaddr && entry_pc < p_end {
            return p_offset.checked_add(entry_pc - p_vaddr);
        }
    }
    None
}

/// Zero a 4 KiB physical page under the current early-boot direct-access model.
#[inline]
pub fn zero_phys_page(ppn : PhysPageNum) {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe {
        core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
    }
}

/// Computes the page-rounded end address for an mmap request.
pub fn mmap_map_end(base : VirtAddr, len : usize) -> MmResult<VirtAddr> {
    let n_pages = len.checked_add(PAGE_SIZE - 1)
                     .ok_or(MmError::InvalidAddress)? /
                  PAGE_SIZE;
    Ok(VirtAddr(base.0
                    .checked_add(n_pages * PAGE_SIZE)
                    .ok_or(MmError::InvalidAddress)?))
}

const MAX_MMAP_SEARCH_PAGES : usize = 1 << 20;

/// Finds the first fully unmapped page range large enough for an mmap request.
pub fn find_free_mmap_base<S>(aspace : &S, cursor : VirtAddr, len : usize) -> MmResult<VirtAddr>
    where S : AddressSpaceOps {
    if len == 0 {
        return Err(MmError::InvalidAddress);
    }
    let n_pages = len.checked_add(PAGE_SIZE - 1)
                     .ok_or(MmError::InvalidAddress)? /
                  PAGE_SIZE;
    let mut base = cursor.ceil_page()
                         .start_addr();
    let mut skipped = 0usize;
    loop {
        if skipped > MAX_MMAP_SEARCH_PAGES {
            return Err(MmError::InvalidAddress);
        }
        let mut free = true;
        for i in 0..n_pages {
            let va = VirtAddr(base.0
                                  .checked_add(i.checked_mul(PAGE_SIZE)
                                                .ok_or(MmError::InvalidAddress)?)
                                  .ok_or(MmError::InvalidAddress)?);
            if aspace.translate_addr(va)?
                     .is_some()
            {
                free = false;
                break;
            }
        }
        if free {
            return Ok(base);
        }
        skipped += 1;
        base = VirtAddr(base.0
                            .checked_add(PAGE_SIZE)
                            .ok_or(MmError::InvalidAddress)?);
    }
}

/// Maps `[start, end)` to freshly allocated zeroed frames.
pub fn map_zeroed_range_with_alloc<S, A>(aspace : &mut S,
                                         allocator : &mut A,
                                         start : VirtAddr,
                                         end : VirtAddr,
                                         perm : PagePerm)
                                         -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    if start.0 >= end.0 {
        return Ok(());
    }
    let mut vpn = start.floor_page();
    let vpn_end = end.ceil_page();
    while vpn.0 < vpn_end.0 {
        map_zeroed_page_with_alloc(aspace, allocator, vpn, perm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(())
}

/// Maps one virtual page to a freshly allocated zeroed frame.
pub fn map_zeroed_page_with_alloc<S, A>(aspace : &mut S,
                                        allocator : &mut A,
                                        vpn : VirtPageNum,
                                        perm : PagePerm)
                                        -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let ppn = allocator.alloc_frame()?;
    zero_phys_page(ppn);
    aspace.map_page_to_ppn(vpn, ppn, perm)
}

/// Maps `[base, end)` to freshly allocated frames filled from `backing`.
pub fn map_range_from_backing<S, A>(aspace : &mut S,
                                    allocator : &mut A,
                                    base : VirtAddr,
                                    end : VirtAddr,
                                    perm : PagePerm,
                                    backing : &[u8])
                                    -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let mut vpn = base.floor_page();
    let vpn_end = end.ceil_page();
    let mut page_index = 0usize;
    while vpn.0 < vpn_end.0 {
        let ppn = allocator.alloc_frame()?;
        fill_phys_page(ppn, page_index, backing);
        aspace.map_page_to_ppn(vpn, ppn, perm)?;
        vpn = VirtPageNum(vpn.0 + 1);
        page_index += 1;
    }
    Ok(())
}

/// Maps `[base, end)` to freshly allocated frames filled by `load_page`.
pub fn map_range_from_loader<S, A, F>(aspace : &mut S,
                                      allocator : &mut A,
                                      base : VirtAddr,
                                      end : VirtAddr,
                                      perm : PagePerm,
                                      mut load_page : F)
                                      -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>,
          F : FnMut(usize, &mut [u8]) -> MmResult<()>
{
    let mut vpn = base.floor_page();
    let vpn_end = end.ceil_page();
    let mut page_index = 0usize;
    while vpn.0 < vpn_end.0 {
        let ppn = allocator.alloc_frame()?;
        let pa = ppn.0 * PAGE_SIZE;
        let page = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        page.fill(0);
        if let Err(e) = load_page(page_index, page) {
            let _ = allocator.dealloc_frame(ppn);
            let _ = aspace.unmap_range_with_alloc(allocator, base, vpn.start_addr());
            return Err(e);
        }
        if let Err(e) = aspace.map_page_to_ppn(vpn, ppn, perm) {
            let _ = allocator.dealloc_frame(ppn);
            let _ = aspace.unmap_range_with_alloc(allocator, base, vpn.start_addr());
            return Err(e);
        }
        vpn = VirtPageNum(vpn.0 + 1);
        page_index += 1;
    }
    Ok(())
}

/// Fills one physical page with the corresponding chunk from `src`.
pub fn fill_phys_page(ppn : PhysPageNum, page_index : usize, src : &[u8]) {
    let pa = ppn.0 * PAGE_SIZE;
    let page = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
    page.fill(0);
    let start = page_index * PAGE_SIZE;
    if start >= src.len() {
        return;
    }
    let end = (start + PAGE_SIZE).min(src.len());
    page[..end - start].copy_from_slice(&src[start..end]);
}

/// Linux `MREMAP_*` flags understood by [`mremap_range`].
pub const MREMAP_MAYMOVE : usize = 1;
pub const MREMAP_FIXED : usize = 2;
pub const MREMAP_DONTUNMAP : usize = 4;
const MREMAP_KNOWN : usize = MREMAP_MAYMOVE | MREMAP_FIXED | MREMAP_DONTUNMAP;

fn region_is_mapped<S : AddressSpaceOps>(aspace : &S,
                                         start : VirtAddr,
                                         end_exclusive : VirtAddr)
                                         -> MmResult<bool> {
    let mut vpn = start.floor_page();
    let vpn_end = end_exclusive.ceil_page();
    while vpn.0 < vpn_end.0 {
        if aspace.translate_addr(vpn.start_addr())?
                 .is_none()
        {
            return Ok(false);
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(true)
}

fn copy_mapped_bytes<S : AddressSpaceOps>(aspace : &S,
                                          src : VirtAddr,
                                          dst : VirtAddr,
                                          len : usize)
                                          -> MmResult<()> {
    let mut offset = 0usize;
    while offset < len {
        let src_va = VirtAddr(src.0
                                 .checked_add(offset)
                                 .ok_or(MmError::InvalidAddress)?);
        let dst_va = VirtAddr(dst.0
                                 .checked_add(offset)
                                 .ok_or(MmError::InvalidAddress)?);
        let src_pa = aspace.translate_addr(src_va)?
                           .ok_or(MmError::NotMapped)?;
        let dst_pa = aspace.translate_addr(dst_va)?
                           .ok_or(MmError::NotMapped)?;
        let page_off = offset % PAGE_SIZE;
        let chunk = core::cmp::min(PAGE_SIZE - page_off, len - offset);
        unsafe {
            core::ptr::copy_nonoverlapping((src_pa.0 + page_off) as *const u8,
                                           (dst_pa.0 + page_off) as *mut u8,
                                           chunk);
        }
        offset += chunk;
    }
    Ok(())
}

fn mremap_relocate<S, A>(aspace : &mut S,
                         allocator : &mut A,
                         old_start : VirtAddr,
                         old_end : VirtAddr,
                         new_size : usize,
                         new_base : VirtAddr,
                         perm : PagePerm,
                         unmap_old : bool)
                         -> MmResult<VirtAddr>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let new_end = mmap_map_end(new_base, new_size)?;
    map_zeroed_range_with_alloc(aspace,
                                allocator,
                                new_base,
                                new_end,
                                perm)?;
    let copy_len = core::cmp::min(old_end.0
                                         .saturating_sub(old_start.0),
                                  new_end.0
                                         .saturating_sub(new_base.0));
    copy_mapped_bytes(aspace, old_start, new_base, copy_len)?;
    if unmap_old {
        aspace.unmap_range_with_alloc(allocator, old_start, old_end)?;
    }
    Ok(new_base)
}

/// Linux `mremap(2)` 语义子集：匿名私有映射 grow/shrink、`MREMAP_MAYMOVE` 搬迁。
pub fn mremap_range<S, A>(aspace : &mut S,
                          allocator : &mut A,
                          old_addr : VirtAddr,
                          old_size : usize,
                          new_size : usize,
                          flags : usize,
                          new_address : VirtAddr,
                          relocation_base : VirtAddr,
                          force_move : bool,
                          perm : PagePerm)
                          -> MmResult<VirtAddr>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    if new_size == 0 {
        return Err(MmError::InvalidAddress);
    }
    if flags & !MREMAP_KNOWN != 0 {
        return Err(MmError::InvalidAddress);
    }
    if flags & MREMAP_FIXED != 0 && flags & MREMAP_MAYMOVE == 0 {
        return Err(MmError::InvalidAddress);
    }
    if flags & MREMAP_DONTUNMAP != 0 && flags & MREMAP_FIXED == 0 {
        return Err(MmError::InvalidAddress);
    }

    let old_start = old_addr.floor_page()
                            .start_addr();
    let old_end = VirtAddr(old_addr.0
                                   .checked_add(old_size)
                                   .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                    .start_addr();
    let new_end = VirtAddr(old_addr.0
                                   .checked_add(new_size)
                                   .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                    .start_addr();

    if !region_is_mapped(aspace, old_start, old_end)? {
        return Err(MmError::NotMapped);
    }

    if flags & MREMAP_FIXED != 0 {
        // 固定地址搬迁：先清空目标区间，再拷贝旧内容
        if new_address.0 % PAGE_SIZE != 0 {
            return Err(MmError::InvalidAddress);
        }
        let dest_start = new_address.floor_page()
                                    .start_addr();
        let dest_end = VirtAddr(new_address.0
                                           .checked_add(new_size)
                                           .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                            .start_addr();
        if dest_start.0 < old_end.0 && dest_end.0 > old_start.0 {
            return Err(MmError::InvalidAddress);
        }
        aspace.unmap_range_with_alloc(allocator, dest_start, dest_end)?;
        map_zeroed_range_with_alloc(aspace,
                                    allocator,
                                    dest_start,
                                    dest_end,
                                    perm)?;
        let copy_len = core::cmp::min(old_end.0
                                             .saturating_sub(old_start.0),
                                      dest_end.0
                                              .saturating_sub(dest_start.0));
        copy_mapped_bytes(aspace, old_start, dest_start, copy_len)?;
        if flags & MREMAP_DONTUNMAP == 0 {
            aspace.unmap_range_with_alloc(allocator, old_start, old_end)?;
        }
        return Ok(new_address);
    }

    if new_end.0 <= old_end.0 {
        // 缩小或等长：截断尾部映射即可
        if new_end.0 < old_end.0 {
            aspace.unmap_range_with_alloc(allocator, new_end, old_end)?;
        }
        return Ok(old_addr);
    }

    if force_move {
        if flags & MREMAP_MAYMOVE == 0 {
            return Err(MmError::InvalidAddress);
        }
        return mremap_relocate(aspace,
                               allocator,
                               old_start,
                               old_end,
                               new_size,
                               relocation_base,
                               perm,
                               true);
    }

    let mut vpn = old_end.floor_page();
    let grow_end = new_end.ceil_page();
    while vpn.0 < grow_end.0 {
        if aspace.translate_addr(vpn.start_addr())?
                 .is_some()
        {
            // 原位增长会与已有映射冲突，需 MAYMOVE 整体搬迁
            if flags & MREMAP_MAYMOVE == 0 {
                return Err(MmError::InvalidAddress);
            }
            return mremap_relocate(aspace,
                                   allocator,
                                   old_start,
                                   old_end,
                                   new_size,
                                   relocation_base,
                                   perm,
                                   true);
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }

    map_zeroed_range_with_alloc(aspace,
                                allocator,
                                old_end,
                                new_end,
                                perm)?;
    Ok(old_addr)
}
