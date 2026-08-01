# K-01 Page-Cache Eviction Failure Report

## Problem and Impact

When the cache was full and selected a dirty victim, `install_page()` and
`install_zero_page()` called `detach_slot_for_reuse()` before lower-layer
writeback. Detaching removed the index entry and cleared the frame's dirty
state. If `PageCacheIo::write_range()` failed, the slot was returned to the
free list without restoring its saved bytes, while the per-file dirty-page map
still retained the old version. A later flush could therefore lose modified
file data silently. Under BuildStorm or I/O pressure this could surface as a
truncated/corrupt artifact or filesystem inconsistency after interruption.

## Implementation

- Dirty victims remain indexed with their data, dirty bit, and version intact
  while the snapshot is written outside the cache lock.
- A successful writeback detaches the frame only if its key and version still
  match the snapshot. A concurrent write keeps the newer frame dirty and makes
  eviction retry with the new version.
- A failed writeback puts the unchanged victim back on the dirty LRU and
  returns the error without losing data.
- Removed the fallback that forcibly cleared an indexed frame when all LRU
  lists were transiently empty; callers now wait for an in-flight victim to
  return.
- No task, filesystem, or public VFS API changed.

Changed file:
`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`.

## Verification

- Page-cache host tests: 12/12 passed.
- Failure-injection tests cover both cache-miss and new-page-write eviction;
  each proves a failed dirty eviction retains the exact payload and succeeds
  on retry or explicit flush.
- New race test modifies the victim during writeback and proves the latest
  version is written before slot reuse.
- `make check` and `make la_check`: passed.
- RISC-V QEMU wrote and synced a 19,379,440-byte file, exceeding the 16 MiB
  cache, then verified SHA-256
  `36bfa3a9b1543b409b7c5bff42d0a6ebce524cdf2d9c57e89690985aa0ae9d83`.
- Converted overlay passed all five `e2fsck -fn` phases with exit code 0.

Logs: `/tmp/wateros-pagecache-eviction-20260801.log` and
`/tmp/wateros-pagecache-eviction-e2fsck-20260801.log`.

Full BuildStorm, iozone, SMP stress, and real block-device failure injection
remain nighttime gates; this repair does not by itself close K-01.
