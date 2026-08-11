# RIO-10 Eventfd Regression Report

## Scope

This validation reuses the existing LTP binaries and their source under
`test_case/ltp-full-20240524`. It covers eventfd read/write errors, readiness,
fork sharing, descriptor flags, and semaphore behavior without adding a
parallel test implementation.

## Cases and Results

- `eventfd01`: initial counter read, empty nonblocking `EAGAIN`, and short-read
  `EINVAL` passed.
- `eventfd02`: counter write/read, overflow `EAGAIN`, short-write `EINVAL`, and
  `UINT64_MAX` rejection passed.
- `eventfd03` and `eventfd04`: `select` read/write readiness transitions passed.
- `eventfd05`: a child update was visible through the inherited eventfd in its
  parent.
- `eventfd2_01` and `eventfd2_02`: `FD_CLOEXEC` and `O_NONBLOCK` flag behavior
  passed.
- `eventfd2_03`: two forked semaphore users completed all reciprocal waits.

The first group produced 17 TPASS results and the second produced 6, with all
eight runner cases reporting `ok=true`.

## Verification Environment

- Kernel: RISC-V64 LTP glibc-only build at commit `c7dd4c0b`.
- QEMU: `virt`, one CPU, 1 GiB RAM, OpenSBI, disposable qcow2 overlays.
- Backing image: `/tmp/wateros-read-mini-20260801.img`.
- Limits: 15 seconds per case; 90 seconds per QEMU session.
- Logs: `/tmp/wateros-eventfd-five-20260801.log` and
  `/tmp/wateros-eventfd2-three-20260801.log`.

No TFAIL, TBROK, timeout, panic, fatal trap, or fd restoration warning was
observed. Full LTP, SMP stress, CAgent, BuildStorm, iozone, and LoongArch64
validation remain outside this daytime test window.
