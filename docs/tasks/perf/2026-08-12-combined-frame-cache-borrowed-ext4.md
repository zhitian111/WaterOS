# Combine physical-frame VFS cache and borrowed ext4 path walk

## Why combine these candidates

The accepted main RISC-V BuildStorm reference is 534.26 s. Two isolated
candidates were functionally correct and directionally positive, but each was
below the roughly 10 s acceptance threshold:

| isolated candidate | result | delta from main |
| --- | ---: | ---: |
| 32 MiB VFS cache payload backed by physical frames | 526.55 s | -7.71 s |
| borrowed another-ext4 path components | 530.42 s | -3.84 s |

Their measured sum would be about 11.55 s, large enough to accept if the gains
remain additive. The mechanisms are also separated: the first removes TLSF
ownership of 8,192 long-lived 4 KiB payloads while preserving cache capacity,
index and replacement; the second removes transient `Vec<String>` and parent
path join allocations inside ext4 pathname walk without changing cache
admission or block I/O.

This is different from the rejected frame-cache + batched-readahead combination
(568.62 s), where readahead directly changed cache residency and polluted the
same replacement domain. This candidate does not change page count, readahead,
LRU, mmap sharing, metadata cache, or block-cache policy.

## Procedure and acceptance

1. Start from current main and cherry-pick only `ff316db0` (physical frame
   payload) and `614b01be` (borrowed ext4 components).
2. Preserve the accepted 8,192-page / 32 MiB VFS cache capacity; the frame
   commit originally tested a larger capacity in later descendants, which is
   explicitly out of scope.
3. Run the vendor ext4 tests used by the borrowed-path experiment, `make check`,
   `make la_check`, and `make all`; verify both kernel artifacts and
   `SCRIPT_BODY_FLAT_BEGIN`.
4. Run one fixed-image RISC-V BuildStorm sample. Accept only a functional first
   result below 524.26 s. A regression or noise-sized result is rejected
   without repetition.
5. Merge only the implementation commits to main after acceptance, then add a
   focused main integration commit/document result and rebuild `make all` so
   main always remains the best validated kernel.

## Result: rejected

- Fixed image SHA-256:
  `ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`
- RISC-V BuildStorm: **548.82 s**; toolchain, minibuild, compile, and judge
  passed with no panic/SIGSEGV/stall/timeout.
- Accepted main: **534.26 s**.
- Delta: **+14.56 s / +2.73% regression**.

The combination does not enter main and is not repeated. The two isolated
results (526.55 s and 530.42 s) did not compose; their apparent 3--8 second
gains were within system noise or changed when combined. This also rejects the
low-priority strategy of accumulating individually sub-threshold candidates
without a shared structural explanation. Future work should require either a
single change above the acceptance margin or fresh diagnostics proving that a
combination removes the same measured bottleneck without adding another cache
or allocator layer.
