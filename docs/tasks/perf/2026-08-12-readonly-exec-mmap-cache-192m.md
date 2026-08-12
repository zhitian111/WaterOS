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
