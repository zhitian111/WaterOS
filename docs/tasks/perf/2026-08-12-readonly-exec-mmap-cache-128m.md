# Readonly executable mmap cache 128 MiB experiment

## Context

The accepted readonly executable mmap physical-page cache reduced the matched
BuildStorm result from 783.00 s to 640.95 s. A separate 300 s diagnostics run
reported 344,064 lookups, 277,389 hits (80.62%), 16,384 resident pages, and
50,123 misses bypassing admission after the 64 MiB cache became full.

The high hit rate validates cross-process reuse of executable file pages. The
large full-cache bypass count is evidence that the cache does not cover the
whole executable working set, but does not prove that every bypassed page will
be reused.

## Hypothesis

Increase only the readonly executable mmap cache capacity from 16,384 pages
(64 MiB) to 32,768 pages (128 MiB). This may retain more repeatedly mapped
compiler, linker, Cargo, and shared-library code pages, reducing file-backed
page faults, VFS reads, copies, and VirtIO traffic.

The additional upper bound is 64 MiB of guest physical frames, 0.39% of the
16 GiB BuildStorm guest memory, plus bounded map metadata. Admission remains
restricted to private, executable, non-writable mappings. Writable/private and
shared mappings remain unchanged. A full cache still bypasses admission; this
experiment does not add eviction scans or change page-table semantics.

## Change and verification

1. Change `MMAP_READONLY_PAGE_CACHE_CAPACITY` from 16,384 to 32,768.
2. Run `make check` and `make all` from `os/`.
3. Verify both Final/default kernel aliases and the script-body marker.
4. Run one matched RISC-V BuildStorm sample with the fixed image and runner.

## Acceptance

The accepted baseline is 640.95 s. Accept only a clear wall-clock improvement
larger than recent roughly 10 s run noise, with compile success and no timeout,
stall, panic, or SIGSEGV. Per the project rule, a clearly successful first run
is sufficient and will not be repeated. Reject a regression or noise-sized
change and record the result without merging the implementation.

## Result

The first and only matched sample passed:

| item | result |
| --- | ---: |
| accepted 64 MiB baseline | 640.95 s |
| 128 MiB candidate | 534.26 s |
| improvement | 106.69 s / 16.65% |
| host wall time | 556.985 s |
| output artifact | 1,681,000 bytes |

All toolchain, minibuild, compile, and judge markers passed. The runner reported
no timeout, stall, panic, or SIGSEGV, and the QEMU process exited successfully.
The candidate kernel SHA-256 was
`9e8b698702477268e5c12de2f247d2ff7302818f33b0c07b19887eefa432fee6`;
the fixed image SHA-256 remained
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/readonly-exec-mmap-cache-128m-a1/result.json`.

The 106.69 s reduction is far beyond the acceptance threshold and supports the
diagnostics-based capacity hypothesis. Per the one-clear-run rule, no second
performance sample was run. Accept the 128 MiB capacity and merge it to main.

## Follow-up saturation diagnostics plan

Before considering another capacity increase, run the accepted 128 MiB kernel
with `cache-layer-diagnostics` for a fixed 300 s window in a separate worktree.
This is not a performance sample. Record the last readonly executable mmap
cache counters and use them only to answer whether the cache reaches 32,768
resident pages and still bypasses substantial misses. Do not test 256 MiB if
the 128 MiB cache is not saturated or the remaining bypass count is small.
