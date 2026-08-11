# Frame-backed 64 MiB file page-cache experiment

## Context

The accepted main kernel completes the fixed-image BuildStorm compile in
534.26 s. Its 300 s cache diagnostics reported 221,006 demand lookups, 565,426
prefetch lookups, 628,045 hits, 158,387 misses, 148,973 clean evictions, and
94,731 evictions of pages that had not been referenced after installation. The
8,192-slot cache was full. This is both a large amount of replacement and a
significant stream component, so capacity alone may help but pollution remains
a risk.

The current cache stores all payload in one 32 MiB `Vec<u8>` allocated from the
128 MiB kernel heap. A historical 48 MiB capacity run stopped making progress
after cagent, consistent with heap pressure or fragmentation. This layout makes
it unsafe to test whether the working set benefits from more resident pages.

## Hypothesis

Move non-test page-cache payload from the kernel heap to individually owned
physical pages, matching Linux's basic page-cache/VM ownership model, and raise
capacity to 16,384 pages (64 MiB). Metadata stays in bounded heap vectors and
the existing index, clean/dirty LRU, writeback, read-ahead, and lifetime rules
remain unchanged.

The change removes a large contiguous heap allocation and may reduce clean
replacement enough to save more than 10 s. It does not yet map cache frames
directly into user page tables; that requires a separate refcount and invalidation
contract. Physical pages are allocated only after the global frame allocator is
initialized. Host unit tests retain heap-backed payload so they do not depend on
kernel boot state.

## Change and verification

1. Add the frame-allocator aggregate as an implementation dependency of the
   page-cache crate.
2. Use `OwnedPhysPage` for kernel cache slots and keep a `cfg(test)` heap
   backing for deterministic unit tests.
3. Increase `FILE_PAGE_CACHE_CAPACITY` from 8,192 to 16,384 pages.
4. Run the page-cache unit tests, normal kernel check, and both architecture
   builds; inspect logs only if a command fails.
5. Verify default/Final kernel hashes and the script-body marker.
6. Run one matched RISC-V BuildStorm sample against the fixed image.

## Acceptance and stop conditions

Accept only if the first matched run succeeds without panic, stall, timeout, or
SIGSEGV and improves the 534.26 s baseline by more than 10 s. A clear first-run
win is sufficient and is not repeated. Reject a regression or noise-sized
change without a second performance run. If frame allocation or boot ordering
fails, revert this candidate rather than weakening allocator ownership rules.

## Result

The first and only matched sample passed all toolchain, minibuild, compile, and
judge markers. It produced the expected 1,681,000-byte artifact and exited
without timeout, stall, panic, or SIGSEGV.

| item | result |
| --- | ---: |
| accepted 32 MiB baseline | 534.26 s |
| frame-backed 64 MiB candidate | 526.55 s |
| improvement | 7.71 s / 1.44% |
| host wall time | 549.418 s |

The candidate kernel SHA-256 was
`32da8526a68a3ec7dc33602fa224e8d38431577976da9424ce32d2732c1b7fbb`;
the fixed image SHA-256 remained
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/frame-backed-page-cache-64m-a1/result.json`.

The 7.71 s change is below the predeclared 10 s acceptance threshold. The
experiment proves that physical-frame backing boots and completes correctly,
but does not show a sufficiently large wall-clock benefit from doubling this
cache. Per the stop rule, do not run a second sample and do not merge the
implementation to main. Preserve this branch as the performance-failed record.
