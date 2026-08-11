# K-10 RISC-V Check Exit-Status Report

## Problem

The `rv_check` Makefile recipe invoked Cargo as `@-cargo check ...`. The leading
`-` instructed Make to ignore a nonzero Cargo exit status. A broken RISC-V build
therefore returned success and printed the same completion message as a valid
build, weakening every task report that used `make rv_check` as a gate.

## Change

File: `os/Makefile`

The ignore-error prefix was removed. The command, feature flags, profile, and
output remain unchanged. This is build infrastructure only and does not affect
kernel or task-module architecture.

## Verification

Negative-path verification used a temporary `cargo` shim that printed a marker
and exited with status 42:

```text
PATH=/tmp/wateros-failing-cargo-20260801:$PATH make rv_check
```

Result:

- Cargo shim exit: 42.
- Make exit: 2 (nonzero).
- The final “RISC-V cargo check complete” message was not printed.
- Log: `/tmp/wateros-rv-check-failure-propagation-20260801.log`.

Positive-path verification:

- `make rv_check`: passed with the real Cargo toolchain.
- `make la_check`: passed, confirming the unchanged LoongArch gate.
- `git diff --check`: passed before commit.

The temporary shim and log are outside the repository and are not committed.
