# Readonly executable mmap cache 192 MiB experiment

## Context

The accepted readonly executable mmap physical-page cache currently holds at
most 32,768 pages (128 MiB) and reduced the matched BuildStorm result from
640.95 s to 534.26 s. A 300 s diagnostics window reached 27,585 resident pages
(about 107.8 MiB), with no full-cache bypass. That window did not cover the
remaining roughly 234 s of the successful compile, so it cannot rule out a
larger executable working set later in the workload.

## Hypothesis

Increase only the readonly executable private mmap cache from 32,768 to 49,152
pages (192 MiB). If the later BuildStorm phases exceed the observed 128 MiB
working set and reuse those pages across compiler/linker processes, the extra
capacity can avoid file-backed page faults and synchronous storage reads.

The extra bound is 64 MiB of guest physical memory (0.39% of the 16 GiB guest).
Admission policy, immutable shared-frame semantics, content-version checks,
and copy-on-write behavior remain unchanged.

## Verification and acceptance

1. Run `make check` and `make all` from `os/`.
2. Verify Final/default RISC-V and LoongArch artifacts and the script marker.
3. Run one matched BuildStorm sample against the fixed image.
4. Compare against the accepted 128 MiB result of 534.26 s.

Accept only a successful improvement larger than the recent roughly 10 s run
noise. A clear first result is final and is not repeated. Reject a regression
or noise-sized improvement without merging the capacity change.

## Result

The first matched sample passed the toolchain, minibuild, compile, artifact,
and judge checks, but completed in 569.98 s. Against the accepted 128 MiB
result of 534.26 s this is a 35.72 s / 6.69% regression. Host wall time was
592.948 s. There was no panic, stall, timeout, or SIGSEGV.

The candidate kernel SHA-256 was
`4ed6615ae41c83cd43a3239fbe005ebc67ae34a1f2bb50e353b5cdf28a874879`;
the fixed image SHA-256 remained
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/readonly-exec-mmap-cache-192m-a1/result.json`.

The larger bound therefore does not expose useful residual capacity in this
workload. The extra resident frames and cache metadata/search footprint cost
more than any later reuse they preserve. Per the predeclared stop condition,
do not repeat this sample and do not merge the capacity change. Preserve this
branch as the performance-failed record; main remains at 128 MiB.
