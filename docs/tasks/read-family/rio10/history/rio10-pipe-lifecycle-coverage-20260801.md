# RIO-10 Pipe Lifecycle Coverage Report

## Scope

This test-only task extends the permanent read-family LTP matrix with pipe
signal and multi-reader close coverage. It does not change kernel behavior or
the task/scheduler architecture.

## Source-Based Selection

The cases were selected from the repository's LTP 20240524 source:

- `read01`: regular-file read count and data.
- `pipe14`: buffered data followed by EOF after the writer closes.
- `pipe08`: writing after the final reader closes returns `EPIPE` and delivers
  exactly one `SIGPIPE`.
- `pipe13`: closing the final writer wakes every blocked reader, tested with 2,
  10, 27, and 100 child readers.

`pipe08` and `pipe13` were added to
`os/scripts/guest_read_family_regression.sh`. Existing defaults already include
`read01` and `pipe14`.

## Verification

All tests ran in RISC-V QEMU using the glibc LTP image, a temporary raw backing
copy, and a disposable qcow2 overlay. Each case had a 15-second limit and each
QEMU invocation had a 75-second limit.

- `read01`: 1 TPASS, exit 0.
- `pipe14`: 1 TPASS, exit 0.
- `pipe08`: `EPIPE` assertion succeeded and `sigpipe_cnt == 1` passed, exit 0.
- `pipe13`: all four reader-count rounds passed, exit 0.
- No TFAIL, TBROK, timeout, panic, or fatal trap was observed.
- Runner syntax: `sh -n os/scripts/guest_read_family_regression.sh` passed.

Logs:

- `/tmp/wateros-read01-pipe14-20260801.log`
- `/tmp/wateros-pipe08-20260801.log`
- `/tmp/wateros-pipe13-20260801.log`

Full LTP, SMP stress, CAgent, and BuildStorm remain deferred to the nighttime
integration gate.
