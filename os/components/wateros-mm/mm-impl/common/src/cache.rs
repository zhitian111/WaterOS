//! 通用只读 ELF/mmap 页缓存：以文件内容身份和版本为键，共享不可变物理页。
//!
//! 缓存锁只保护索引与引用计数；实际文件读取和页清零在锁外执行，避免 I/O 阻塞其它缺页路径。

use super::*;

const ELF_READONLY_PAGE_CACHE_CAPACITY : usize = 16_384;
#[cfg(feature = "cache-layer-diagnostics")]
const ELF_CACHE_DIAGNOSTIC_REPORT_LOOKUPS : u64 = 1 << 14;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ElfReadonlyPageKey {
    /// 挂载实例代数；卸载后重新挂载即使 inode 数值复用也不会命中旧页。
    mount_generation : u64,
    /// 挂载对象标识。
    mount_id : u64,
    /// 文件节点标识。
    node_id : u64,
    /// 文件内容版本；写入或截断后必须变化。
    content_version : u64,
    /// ELF 段虚拟基址。
    vbase : usize,
    /// 段在文件中的起始偏移。
    p_offset : usize,
    /// 段文件部分长度，避免不同段参数错误共享同一页。
    filesz : usize,
    /// 当前 VMA 起点。
    vma_start : usize,
    /// 请求页对应的文件偏移。
    file_offset : usize,
}

struct ElfReadonlyPageEntry {
    /// 缓存页物理页号；缓存本身持有一份引用。
    ppn : PhysPageNum,
    /// 逻辑访问时钟，用于容量达到上限时淘汰最久未使用项。
    last_used : u64,
    /// 保持文件身份对象存活，避免缓存项引用已销毁的版本状态。
    _identity : VfsFileContentIdentity,
}

struct ElfReadonlyPageCache {
    /// 由完整内容身份和段参数索引的只读页。
    entries : BTreeMap<ElfReadonlyPageKey, ElfReadonlyPageEntry>,
    /// 访问时钟；回绕只影响淘汰顺序，不影响内容正确性。
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

/// 加载或复用一个不可变 ELF 页。返回帧由调用方映射持有一份引用，缓存另持一份引用。
/// 加载回调在缓存锁外运行，因此并发缺页可能发生重复 I/O；重新取得锁后只发布一个缓存项，
/// 其余重复帧会按引用计数释放。
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

/// 定向检查缓存与引用计数；调用者必须先初始化全局帧分配器。
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

const MMAP_READONLY_PAGE_CACHE_CAPACITY : usize = 32_768;
#[cfg(feature = "cache-layer-diagnostics")]
const MMAP_CACHE_DIAGNOSTIC_REPORT_LOOKUPS : u64 = 1 << 14;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MmapReadonlyPageKey {
    /// 挂载代数，阻止卸载后复用 inode 命中旧页。
    mount_generation : u64,
    /// 挂载对象标识。
    mount_id : u64,
    /// 文件节点标识。
    node_id : u64,
    /// 文件内容版本。
    content_version : u64,
    /// 页对齐的文件偏移。
    file_offset : usize,
    /// 映射观察到的文件大小；尾页零填充语义随大小变化。
    mapping_file_size : usize,
}

struct MmapReadonlyPageEntry {
    /// 缓存页物理页号。
    ppn : PhysPageNum,
    /// 逻辑访问时钟；仅用于诊断和命中更新。
    last_used : u64,
    /// 延长内容版本身份的生命周期。
    _identity : VfsFileContentIdentity,
}

struct MmapReadonlyPageCache {
    /// 文件映射只读页缓存索引。
    entries : BTreeMap<MmapReadonlyPageKey, MmapReadonlyPageEntry>,
    /// 访问逻辑时钟。
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

/// 加载或复用私有文件映射中的不可变页。缓存和返回映射各持有一份帧引用。
/// 文件 I/O 在缓存锁外运行；内容版本变化时使用新键重试，避免把旧内容安装到新映射。
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
                    // 保留已建立的热点集合，只把新页交给当前映射。普通文件映射可能包含数百万个
                    // 只访问一次的页；每次缺页都 O(n) 扫描 LRU 淘汰项会比暂不接纳新页更昂贵。
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

/// 定向检查通用 mmap 缓存与引用计数；调用者必须先初始化帧分配器。
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
