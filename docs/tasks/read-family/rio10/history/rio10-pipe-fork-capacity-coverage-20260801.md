# RIO-10 Pipe Fork and Capacity Coverage Report

## Scope

This test-only task adds two existing LTP cases to the permanent read-family
runner. It validates pipe inheritance across `fork` and nonblocking capacity
behavior without changing kernel or task-module code.

## Cases

- `pipe10`: a child inherits the parent's pipe descriptors and reads the full
  payload written before `fork`.
- `pipe12`: a full nonblocking pipe returns `EAGAIN`; larger writes to empty and
  non-empty pipes make progress; `FIONREAD` reports the buffered byte count.

Both cases come from `test_case/ltp-full-20240524` and were added to
`os/scripts/guest_read_family_regression.sh`.

## Verification

The cases ran together in RISC-V QEMU with a temporary raw backing copy, a
disposable qcow2 overlay, a 15-second per-case timeout, and a 75-second
whole-machine timeout.

- `pipe10`: 1 TPASS; child read count matched the 27-byte parent write.
- `pipe12`: 6 TPASS; full-pipe write returned `EAGAIN`, both larger-write cases
  succeeded, and all `FIONREAD` checks reported 65536 bytes.
- Runner result: `passed=2 failed=0 missing=0`.
- No TFAIL, TBROK, timeout, panic, or fatal trap was observed.
- `sh -n os/scripts/guest_read_family_regression.sh`: passed.

Log: `/tmp/wateros-pipe10-pipe12-20260801.log`.

Full LTP, SMP stress, CAgent, and BuildStorm remain part of the nighttime gate.
