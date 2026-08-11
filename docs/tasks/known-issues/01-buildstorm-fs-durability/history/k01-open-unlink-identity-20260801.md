# K-01 Open-Unlink Object Identity Report

## Problem

The page cache is keyed by path. `unlink_path()` previously purged that path
regardless of whether unlink succeeded or whether file handles remained open.
After unlink and same-name recreation, an old fd read the new file:

```text
OPEN_UNLINK_VERIFY ok=false old='new-object' new='new-object'
```

Dirty bytes could also be discarded. The existing detached mode did not help
because its buffer started empty and was activated only after a later
path-based operation returned `NotFound`.

LTP `unlink07` additionally showed that a regular-file intermediate component
returned `ENOENT` instead of `ENOTDIR`.

## Implementation

- Independent opens of one linked path share an internal weakly registered
  detached backing; the registry does not extend handle lifetime.
- Before unlink, VFS snapshots the logical file contents through the page
  cache, including dirty bytes. The snapshot is committed only after the
  backend unlink succeeds.
- Successful unlink removes the path registration and cache entry, so a new
  same-name open gets an independent object. Old fds continue sharing reads
  and writes through their detached backing.
- Detached size, write, and truncate operations no longer mutate the old
  path-key cache.
- Failed unlink no longer purges cache state.
- `unlinkat` now validates non-directory intermediate components before
  dispatch and returns `ENOTDIR`.
- No task-module or public VFS API changed.

## Verification

- Baseline reproduced old-fd aliasing; the fixed run returned
  `old='old-object' new='new-object'`.
- Two independent pre-unlink opens observed `changed-old` after one old fd
  wrote it, while the recreated path remained `new-object`.
- A dirty pre-unlink write was preserved as `dirty-before-unlink`; the new path
  remained independent.
- LTP `unlink05`, `unlink07`, and `unlinkat01`: 3/3 runner cases passed,
  including 15 TPASS results and the corrected `ENOTDIR` assertion.
- `make check`, `make la_check`, and the RISC-V LTP kernel build passed.
- The dirty-unlink overlay passed all five `e2fsck -fn` phases, exit code 0.

Logs: `/tmp/wateros-open-unlink-baseline-20260801.log`,
`/tmp/wateros-open-unlink-shared-20260801.log`,
`/tmp/wateros-open-unlink-dirty-20260801.log`, and
`/tmp/wateros-unlink-ltp-fix-20260801.log`.

## Remaining Limits

The existing detached backing cap is 16 MiB, so unlink of a larger still-open
file can return an I/O error rather than consume unbounded kernel memory.
Open-handle identity across rename/replacement still needs an inode-based or
redirected-backing design. Full BuildStorm, iozone, and SMP races remain
nighttime gates.
