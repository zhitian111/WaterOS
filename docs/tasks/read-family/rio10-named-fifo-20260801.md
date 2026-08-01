# RIO-10 Named FIFO Completion Report

## Scope

This task closes the named FIFO failures exposed by LTP `read03` and `open06`.
It covers FIFO inode metadata, FIFO open/read/write lifecycle, nonblocking errno,
and cleanup through `O_DIRECTORY`. No task or scheduler interface was changed.

## Failures

- `mknod(S_IFIFO)` on ramfs discarded the inode type bits, so `stat` reported a
  character device and `read03` stopped with `TBROK`.
- VFS had anonymous pipe handles but no filesystem FIFO object shared by inode.
- `O_NONBLOCK` was applied only after `open`, too late for FIFO open semantics.
- LTP `open06` reached `TPASS` for `ENXIO` but hung during temporary-directory
  cleanup: `open(fifo, O_DIRECTORY)` incorrectly entered a blocking FIFO read
  open instead of returning `ENOTDIR`.
- another-ext4 did not expose full inode type bits or implement `mknod`.

## Implementation

- Ramfs and another-ext4 preserve special inode type bits; another-ext4 creates
  regular, FIFO, socket, and zero-rdev device inodes through `generic_create`.
- The pipe implementation now supports a named backing object with reopenable
  reader/writer counts and no hidden sentinel endpoints.
- fd-session shares FIFO state by `(mount_id, inode)` through weak references,
  implements read/write/O_RDWR handles, and removes registry entries after the
  final handle is dropped.
- Blocking opens wait on existing pipe waitqueues. Nonblocking writer open with
  no reader returns `ENXIO`; interruption returns `EINTR`.
- VFS open flags now carry `O_NONBLOCK`, and `O_DIRECTORY` rejects non-directory
  nodes before special-handle dispatch.
- The permanent guest runner now includes `open06`, `read03`, and `read04`.

## Verification

- `make rv_check`: Cargo completed successfully.
- `make la_check`: completed successfully.
- `make kernel-rv-ltp-glibc`: completed successfully.
- RISC-V QEMU, default tmpfs: `read03` and `read04` both passed, runner result
  `passed=2 failed=0`.
- RISC-V QEMU, `TMPDIR=/glibc` on another-ext4: `read03` passed.
- LTP `open06` passed and exited normally on both tmpfs and another-ext4,
  confirming `ENXIO` and the `O_DIRECTORY` cleanup fix.
- Logs:
  - `/tmp/wateros-read-mini-fifo-postaudit-20260801.log`
  - `/tmp/wateros-read-root-fifo-20260801.log`
  - `/tmp/wateros-open06-tmpfs-fix-20260801.log`
  - `/tmp/wateros-open06-root-postaudit-20260801.log`

All runs used temporary backing-image copies plus qcow2 overlays and 75-second
whole-machine limits. Full LTP and stress suites remain deferred to nighttime.

## Follow-up Found

`make rv_check` currently prefixes Cargo with `-`, so Make reports success even
when Cargo fails. This was detected because the first check contained a compiler
error despite a zero Make exit code. It must be fixed as a separate
infrastructure task and commit.
