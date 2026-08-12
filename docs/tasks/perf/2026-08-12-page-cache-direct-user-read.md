# Page-cache direct user-read experiment

## Context

The accepted main kernel completes the fixed-image BuildStorm compile in
534.26 s. Its post-RX-cache profile still shows the allocator, zero fill, and
memory-copy paths among the hottest kernel work. A regular-file `read(2)` now
stages up to `SYSCALL_IO_MAX` (4 MiB) in this sequence:

```text
page cache -> zero-filled temporary Vec -> userspace
```

`PagedPreparedRead::acquire` allocates and zeroes the entire available range,
copies cached pages into it, and `sys_read` then copies it again. This preserves
partial-EFAULT and shared-open-description offset semantics, but pays one
large TLSF allocation, a full-range clear, and two data copies. It is an early
bring-up design rather than a Linux-like buffered-read path.

The cache-hit index itself is not linear: both the readonly executable mmap
cache and normal file page cache use `BTreeMap`, so lookup is O(log n), and the
normal file cache uses an intrusive O(1) LRU. This experiment must not replace
the temporary data buffer with repeated key lookup on every destination
fragment.

## Hypothesis and design

For non-detached regular files, install and pin the requested cache slots while
the read reservation is active. Record each resolved slot once in a compact
`Vec<usize>`, then expose the pinned page slices to a sink-style read-lease API.
The syscall sink performs the existing page-fault/COW/permission checks and
copies each cache slice directly into the user physical page:

```text
page cache -> userspace
```

The full-size staging `Vec`, its zero fill, and one data copy disappear. Slot
lookup remains once per source page at O(log n); the copy phase indexes the
already resolved slot in O(1). The page-cache payload remains the accepted
32 MiB contiguous pool so this experiment does not mix in the separately
rejected physical-page backing/capacity change.

Pinned slots are removed from eviction eligibility until the lease is finished
or dropped. Invalidation removes their key but defers slot reuse until the last
pin is released. Cache bytes are visited only while the cache-state lock is
held, preserving Rust aliasing rules; each critical section is limited to one
source-page fragment. Detached files and non-file descriptors keep their
existing staged representation.

The sink reports exact prefix progress. `finish` advances the shared file
offset by only the bytes that reached userspace, so a cross-page EFAULT retains
the current Linux-compatible partial-read behavior. `readv(2)` uses the same
chunk API and preserves iovec scattering semantics.

## Implementation and verification

1. Add pin/unpin state and invariants to the normal page cache, including
   invalidate/truncate handling and focused host tests.
2. Add an object-safe chunk sink to `VfsReadLease`; keep a default staged-data
   implementation for existing pipe/socket/device leases.
3. Return a pinned page-cache lease from `PagedPreparedRead` and update
   `read(2)`/`readv(2)` to consume chunks directly.
4. Run focused host tests, `make check`, and `make all`; inspect logs only on
   failure. Verify both Final artifacts and `SCRIPT_BODY_FLAT_BEGIN`.
5. Run one fixed-image RISC-V BuildStorm sample and compare it with 534.26 s.
6. If accepted, run one post-change 300 s PC-hot diagnostic to confirm that the
   allocator/clear/copy hot paths fell; diagnostics are not wall-clock scores.

## Acceptance and stop conditions

Accept a first successful sample with a clear improvement beyond the recent
roughly 10 s run noise, provided all toolchain, minibuild, compile, artifact,
and judge markers pass without panic, stall, timeout, SIGSEGV, or data error.
A clear first-run win is not repeated. Reject a regression or noise-sized
change without a second run. Any partial-EFAULT offset error, pinned-slot reuse,
dirty-data loss, lock-order cycle, or architecture build failure stops the
candidate before performance testing.
