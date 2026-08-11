# RIO-10 Vector and Extended Pipe Regression Report

## Scope

This test-only task expands the permanent short runner with existing LTP pipe
cases and validates the existing vector/positional read set. No kernel or task
module interfaces were changed.

## Vector and Positional Reads

The RISC-V QEMU run covered `readv01`, `readv02`, `pread01`, `pread02`,
`preadv01`, and `preadv02`. All six runner cases passed, producing 30 TPASS
results. Observed errno checks included `EBADF`, `EFAULT`, `EINVAL`, `EISDIR`,
and `ESPIPE`; successful cases covered zero vectors, multiple vectors, short
input, explicit offsets, and unchanged sequential positioning.

## Extended Pipe Coverage

The run covered `pipe01`, `pipe02`, `pipe04`, `pipe05`, `pipe09`, and `pipe11`:

- basic data transfer and bad-user-pointer `EFAULT` passed;
- SIGPIPE propagation and killing blocked writers passed;
- two-process writes preserved all expected bytes;
- `pipe11` passed its 1, 2, 3, 4, 10, and 50 child reader variants.

These six cases are now part of `os/scripts/guest_read_family_regression.sh`.
`pipe15` is intentionally excluded because it requires Linux pipe-user page
soft-limit procfs controls and creates a limit-sized global pipe population.

## Verification

- Kernel: RISC-V64 LTP glibc-only build at commit `7dcd8ecc`.
- QEMU: `virt`, one CPU, 1 GiB RAM, OpenSBI, disposable qcow2 overlays.
- Limits: 15 seconds per case; 100/110 seconds per QEMU session.
- Logs: `/tmp/wateros-vector-positional-20260801.log` and
  `/tmp/wateros-pipe-extended-20260801.log`.
- `sh -n os/scripts/guest_read_family_regression.sh`: passed.

No TFAIL, TBROK, timeout, panic, fatal trap, or fd restoration warning was
observed. Full workload and SMP stress gates remain deferred to the nighttime
window.
