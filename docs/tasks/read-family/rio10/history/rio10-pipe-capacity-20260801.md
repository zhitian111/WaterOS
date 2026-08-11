# RIO-10 Pipe Capacity Compatibility Report

## Scope

This change fixes the `fcntl(F_SETPIPE_SZ)` behavior exercised by LTP
`pipe2_04`. It is a focused syscall compatibility fix and does not change the
task scheduler, wakeup protocol, or pipe data path.

## Problem

WaterOS rejected requested pipe sizes below one page with `EINVAL`. Linux
instead raises such requests, including zero, to one page. This caused
`pipe2_04` to stop before checking nonblocking writes and blocked-writer
behavior.

## Implementation

File:

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fcntl.rs`

`normalize_pipe_size()` now:

1. Normalizes zero and sub-page requests to one page.
2. Converts the request to pages and rounds the page count up to a power of
   two, matching the pipe ring allocation model.
3. Rejects requested or rounded capacities above the existing 1 MiB limit
   with `EPERM`.
4. Uses checked arithmetic for rounding and byte conversion.

Local tests cover zero, sub-page, exact-page, non-power-of-two, and over-limit
requests.

## Verification

- `make rv_check`: passed.
- `make la_check`: passed; pre-existing unused-code warnings remain.
- `make kernel-rv-ltp-glibc`: passed.
- Short QEMU run of `/glibc/ltp/testcases/bin/pipe2_04`: passed with two
  `TPASS` results (full nonblocking pipe returns `EAGAIN`; writer blocks until
  space is available).
- QEMU log: `/tmp/wateros-pipe-size.log`.
- The temporary guest runner was removed, and `/glibc/ltp_testcode.sh` was
  restored and compared byte-for-byte with its saved original.

Full LTP and stress suites were intentionally not run during daytime. They
remain part of the nighttime RIO-10 integration pass.
