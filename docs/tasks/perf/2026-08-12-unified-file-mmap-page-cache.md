# Unified VFS file-page and read-only mmap cache experiment

## Context

The accepted main RISC-V BuildStorm time is 534.26 s. Its largest improvement
came from sharing private RX mmap pages across repeated cargo/rustc/linker and
shared-library mappings. The current implementation, however, still maintains
two copies of a hot file page:

```text
VFS page-cache byte slot
  -> mmap fault allocates and clears another frame
  -> copies 4 KiB from VFS into that frame
  -> maps the second frame into one or more address spaces
```

The VFS file page cache is a 32 MiB contiguous heap byte pool. The accepted RX
mmap cache is a separate 128 MiB physical-frame cache. Generic read-only
sharing extensions were weak or regressive because they added another index,
another admission policy, and another retained copy. Linux instead makes the
file page cache itself the mapping source for clean file-backed PTEs.

## First-stage design

Convert the existing 8,192-slot VFS cache payload to individually owned
physical frames without changing its capacity or replacement policy, then
allow a file handle to pin the exact clean cached page for a read-only private
mmap fault:

1. A cache slot owns one frame reference. Returning its PPN to a mapping first
   increments the allocator refcount; each PTE owns that additional reference.
2. Cache eviction removes the index/LRU entry and drops only the cache's
   reference. Existing PTE mappings remain valid until their address spaces
   unmap them.
3. Before modifying a cached page (`write`, zero-fill, truncate tail, or slot
   reuse), check its frame refcount. If mappings still reference it, allocate a
   new frame, copy the old bytes, replace the cache-owned frame, and modify the
   new page. This preserves `MAP_PRIVATE` snapshot semantics.
4. Expose the page through the existing `VfsIoHandle`/`DemandPageLoader`
   lifetime boundary using stable file identity and page-aligned offsets. A
   cache miss loads through the normal `FsPageIo`. The accepted 128 MiB RX
   cache remains as a hot-page index and owns a reference to the same frame,
   rather than allocating and copying an independent payload. This preserves
   its proven residency window after the 32 MiB VFS cache evicts its reference.
5. Initially admit only the already accepted
   `MAP_PRIVATE && PROT_EXEC && !PROT_WRITE` class. This isolates the benefit of
   removing the duplicate VFS-to-mmap copy while preserving the proven RX
   workload set. Broader readonly and writable `MAP_SHARED` mapping are later
   stages, not silently folded into this experiment.
6. Keep host unit tests on their existing heap byte backing. Kernel builds use
   the physical-page feature through the fs-bridge dependency.

## Correctness invariants

- Never expose a dirty or partially installed cache page as immutable shared
  backing.
- File write/truncate advances the existing content identity version. A
  loader whose captured version no longer matches must not pin a newer page
  under an older mapping snapshot.
- Page-cache I/O remains outside the global cache lock. Pinning happens only
  after a second exact-key check.
- A failed PTE installation releases the mapping reference. Cache eviction,
  reset, mount-generation change, and shutdown release their ownership exactly
  once.
- `mprotect(W)` continues through the existing private-frame check and copies
  a shared page before enabling write permission.
- No page-table reverse-map or cross-address-space invalidation is required in
  this stage because cached pages are only mapped private and immutable.

## Implementation and verification

1. Reuse the previously validated `OwnedPhysPage` backing but retain the main
   capacity of 8,192 pages.
2. Add cache-side pin/COW helpers and focused tests for stable-key lookup,
   dirty rejection, eviction lifetime, and write/truncate isolation.
3. Add a VFS handle hook for immutable page pinning and connect
   `VfsMmapPageLoader::load_shared_page` to it for RX mappings.
4. Run affected host tests, `make check`, `make la_check`, and `make all`;
   verify both Final artifacts and `SCRIPT_BODY_FLAT_BEGIN`.
5. Run one fixed-image RISC-V BuildStorm sample against 534.26 s. Only after a
   wall-clock acceptance run may a focused diagnostic confirm eliminated copy
   and old RX-cache traffic.

## Acceptance and stop conditions

Accept a first successful run below 524.26 s with all required markers and no
panic, SIGSEGV, timeout, stall, refcount error, stale data, filesystem error, or
COW violation. A clear first-run win is sufficient and is not repeated. Reject
a noise-sized result or regression without a second run. Any ownership or
invalidation ambiguity stops the candidate before performance testing.
