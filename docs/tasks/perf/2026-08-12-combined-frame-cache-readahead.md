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
