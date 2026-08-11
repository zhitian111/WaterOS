# Ext4 batched read-ahead experiment

## Context

The accepted main kernel completes the fixed-image BuildStorm compile in
534.26 s. Its post-RX-cache 300 s profile still attributes about 1.296 billion
guest instructions to `VirtQueue::add_notify_wait_pop`. Block diagnostics in
the same window report 104,463 backend calls for 835,479 backend 512-byte
blocks: almost exactly one 4 KiB ext4 block per synchronous VirtIO request.

The VFS page cache advertises an eight-page (32 KiB) sequential read-ahead
stride, but implements it as eight independent `install_page` calls. The
active `another_ext4` reader also loops over one 4 KiB block at a time, and its
block-device trait exposes only `read_block`. Consequently the outer block
cache's existing contiguous-miss merge can never combine the requests, even
when an ext4 extent is physically contiguous.

## Hypothesis

Carry one contiguous request through all three layers:

1. issue a single lower `PageCacheIo::read_range` for an eight-page read-ahead
   window and publish the returned pages into the existing page cache;
2. group adjacent logical blocks that map to adjacent physical blocks in
   `another_ext4::Ext4::read`;
3. add a default-compatible `BlockDevice::read_blocks` method to
   `another_ext4`, override it in WaterOS's adapter, and let the existing outer
   `CachingBlockDevice::read_blocks` merge contiguous misses into one VirtIO
   request.

This does not change demand-read results, cache keys, dirty-page writeback,
extent semantics, or the synchronous block API. Non-contiguous extents and
holes split the run. The vendor block cache remains authoritative: a bulk
device read is overlaid by cached data (including dirty data) before returning,
then clean misses are admitted exactly as single-block reads are today.

## Verification

1. Add unit tests proving the vendor block cache overlays hits and performs one
   backend bulk call for a clean contiguous miss run.
2. Add a page-cache test proving sequential read-ahead uses one lower range
   read and returns the same bytes.
3. Run the affected host tests, normal kernel check, and both architecture
   builds; inspect logs only on failure.
4. Verify default/Final hashes and the script-body marker.
5. Run one matched fixed-image RISC-V BuildStorm sample.

## Acceptance and stop conditions

Accept only if the first matched run passes all markers without timeout,
stall, panic, SIGSEGV, or data error and improves the 534.26 s baseline by more
than 10 s. A clear first-run win is sufficient and is not repeated. Reject a
regression or noise-sized change without a second performance run. Any dirty
cache coherency ambiguity, short-read bug, or non-contiguous extent error stops
the candidate before performance testing.

