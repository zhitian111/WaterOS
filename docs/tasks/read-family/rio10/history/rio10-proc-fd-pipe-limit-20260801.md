# RIO-10 Proc FD and Pipe Limit Report

## Problem

LTP `pipe07` aborted before exercising the pipe limit because
`opendir("/proc/self/fd")` returned `ENOENT`. The kernel procfs exposed common
per-process files but did not implement the dynamic fd directory.

## Implementation

- Added the procfs `TaskFdLookup` callback contract. It uses numeric task IDs so
  the procfs API remains independent of the task and VFS crates.
- Added a lock-bounded VFS snapshot that enumerates occupied fd slots without
  acquiring individual open-file-description locks or creating missing tables.
- Added `/proc/<pid>/fd` directory nodes and numeric symlink entries, including
  support for the existing dynamic `self` PID lookup.
- Registered the callback from both normal and bootstrap procfs mount paths.
- Added LTP `pipe06` and `pipe07` to the permanent short read-family runner.

Relevant files are under `os/components/wateros-fs/fs-procfs/`,
`os/components/wateros-vfs/`, and
`os/components/wateros-vfs/vfs-impl/impl-fd-session/`.

## Verification

- `make check`: passed for the RISC-V configuration.
- `make kernel-rv-ltp-glibc`: passed.
- `pipe06` baseline: opened 1020 descriptors and received `EMFILE` at the
  configured limit, TPASS.
- `pipe07` after this repair: 2 TPASS; both `errno == EMFILE` and the expected
  count of 1020 pipe descriptors matched.
- The `pipe07` QEMU run completed in 2.6 seconds with a 15-second case timeout
  and a 75-second whole-machine timeout.
- No TFAIL, TBROK, timeout, panic, or fatal trap was observed.

Full LTP, SMP stress, CAgent, BuildStorm, and iozone were intentionally not run
during the daytime verification window.
