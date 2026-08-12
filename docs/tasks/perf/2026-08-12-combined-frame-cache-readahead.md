# Combined frame-backed page-cache and ext4 read-ahead experiment

## Context

The accepted main kernel completes the fixed-image RISC-V BuildStorm compile
in 534.26 s. Two isolated candidates were functionally correct and measured in
the positive direction, but each missed the predeclared 10 s acceptance bar:

- frame-backed 64 MiB file page cache: 526.55 s, 7.71 s faster;
- end-to-end ext4 batched read-ahead: 530.31 s, 3.95 s faster.

Neither result is independently strong enough to merge. However, they address
different costs: the first reduces page-cache replacement and removes a large
heap allocation, while the second reduces synchronous VirtIO request count for
contiguous sequential input. Their effects may compose. This is deliberately a
low-priority validation of the user's observation that several individually
small wins can become material together.

## Hypothesis and scope

Combine only the implementation commits from those two positive experiments on
current main. Do not add the historically regressive metadata, negative-dentry,
lookup-FIFO, direct-user-copy, larger mmap-cache, or IRQ candidates.

The expected best case is roughly additive (about 11.66 s), but overlap in
cache misses and read-ahead means the measured gain may be smaller. The
combination is accepted only on its own result; the two historical deltas are
not summed as evidence.

## Verification

1. Integrate the 64 MiB physical-frame page-cache backing and the contiguous
   eight-page read-ahead pipeline, resolving only their shared page-cache code.
2. Run the page-cache and `another_ext4` narrow tests, `make check`, and both
   architecture Final builds. Read logs only if a command fails.
3. Verify the Final script-body marker and fixed image identity.
4. Run one fixed-image RISC-V BuildStorm sample against the 534.26 s main
   baseline.

## Acceptance and stop conditions

Accept a first successful run below 524.26 s with all toolchain, minibuild,
compile, artifact, and judge markers and no panic, stall, timeout, SIGSEGV, or
filesystem error. A clear first-run win is sufficient. Reject a result within
the noise band or slower than main without a repeat. Do not merge either
component merely because their old isolated numbers were positive.

## Result (rejected)

The two implementation commits combined cleanly and passed the page-cache and
`another_ext4` narrow tests, `make check`, `make la_check`, and `make all`.
Both Final kernels were produced and the RISC-V image contained
`SCRIPT_BODY_FLAT_BEGIN`.

The first and only fixed-image sample passed every toolchain, minibuild,
compile, artifact, and judge marker. It produced the expected 1,681,000-byte
artifact and had no panic, SIGSEGV, stall, timeout, or filesystem error.

| item | result |
| --- | ---: |
| accepted main baseline | 534.26 s |
| combined candidate | 568.62 s |
| regression | 34.36 s / 6.43% |
| host wall time | 592.579 s |

The candidate kernel SHA-256 was
`2055e058587106f06f571c557bebc09a67ace883faa4d984561eb029c5277f05`;
the fixed image SHA-256 was
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/combined-frame-cache-readahead-a1/result.json`.

The isolated deltas were not additive. The likely interaction is that doubling
page-cache residency changes replacement behavior so the eight-page prefetch
window retains more one-shot compiler input and performs more speculative I/O;
the larger cache therefore amplifies read-ahead pollution instead of merely
preserving useful refaults. This is an inference from the direction and scope
of the two changes, not a post-change plugin measurement; because the candidate
is already a clear wall-clock rejection, an ineligible diagnostic run is not
justified.

Do not merge this combination or repeat the run. Main remains the accepted
534.26 s kernel. Future combinations must be measured as combinations rather
than accepting the arithmetic sum of isolated improvements.
