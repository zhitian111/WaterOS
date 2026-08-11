# Non-executable readonly mmap second-hit admission experiment

## Evidence and failed approaches

The accepted private RX physical-page cache is the largest current optimization:
783.00 s fell to 640.95 s, then its 128 MiB capacity reduced the result to
534.26 s. At 300 s the RX cache had a 91.97% hit rate and 27,585 resident pages.

Caching every private readonly file mapping previously regressed to 932.23 s.
That implementation admitted one-shot `.rmeta`, archives, and artifacts, used
an O(n) victim scan after filling, and shared pages later made writable. The
current cache already removed the victim scan, but an executable-mode proxy for
rodata still regressed to 548.33 s because it admitted one-shot produced
executables and permission-transition pages.

The block cache's accepted second-hit admission demonstrates the useful policy:
record a miss cheaply, but allocate persistent cache space only after the same
identity is faulted again. This matches Linux workingset/refault reasoning and
directly targets the remaining failure mode instead of retrying broad admission.

## Candidate design

Keep the accepted RX cache unchanged: private, executable, non-writable mmap
pages enter its 128 MiB cache on their first fault.

Add a separate cache for private, non-executable, non-writable file mappings:

- exact entry key remains mount generation/id, node id, content version, page
  offset, and mapping file-size snapshot;
- capacity is 16,384 pages (64 MiB), independent from the RX hot set;
- first miss loads a private mapping page and records only a 64-bit fingerprint
  in a fixed direct-mapped 16,384-slot ghost history;
- a later exact-key miss whose fingerprint is still present may install the
  loaded page; subsequent faults share the cached PPN;
- collisions can only admit an extra page, never return wrong content, because
  the physical cache lookup remains an exact BTree key;
- full cache misses bypass admission with no O(n) victim scan.

The existing content-version retry, cache/mapping frame references, I/O outside
the lock, race winner, and `mprotect(W)` private-copy checks remain in force.
`MAP_SHARED`, initially writable private mappings, and unstable handles are not
shared. The mm API-v0 contract is unchanged; only the internal mm aggregate and
VFS mmap loader gain the second-hit selection.

## Verification and acceptance

Extend the directed self-test to prove: first generic readonly fault is private,
second fault installs, third fault reuses without I/O, and frame references are
balanced. Run normal and `cache-layer-diagnostics` checks, `make all`, and verify
both architecture aliases plus script markers.

Run one matched full RISC-V BuildStorm sample. The accepted baseline is
534.26 s. Accept only a successful result clearly more than roughly 10 s faster,
with no timeout, stall, panic, or SIGSEGV. A clear first-run win is sufficient;
a regression or noise-sized result is rejected without a second run or merge.

## Result

The first and only matched sample passed but did not reach the acceptance line:

| item | result |
| --- | ---: |
| accepted RX-only baseline | 534.26 s |
| readonly second-hit candidate | 529.26 s |
| difference | -5.00 s / -0.94% |
| host wall time | 552.016 s |
| output artifact | 1,681,000 bytes |

Normal and diagnostics checks, RV/LA builds, and all BuildStorm markers passed.
There was no timeout, stall, panic, or SIGSEGV. The candidate kernel SHA-256
was `146fcd952390a85a3964729198cd7e6bcdc9fd2b7b765220c15db31f6e126d26`;
the fixed image SHA-256 remained
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/readonly-mmap-second-hit-a1/result.json`.

The 5.00 s improvement is below the declared roughly 10 s noise threshold.
Do not run a second sample and do not merge the implementation. Together with
the executable-mode rodata regression, this shows that non-RX mmap sharing is
not the next major wall-clock axis even with scan-resistant admission.
