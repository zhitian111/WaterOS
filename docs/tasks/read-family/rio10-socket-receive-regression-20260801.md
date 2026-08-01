# RIO-10 Socket Receive Regression Report

## Scope

This validation uses the existing LTP `socketpair01`, `socketpair02`, `recv01`,
`recvfrom01`, and `recvmsg01` binaries. It verifies the socket read-side error
and flag surface without introducing custom guest test code.

## Results

All five runner cases completed successfully and produced 36 TPASS results:

- `socketpair01`: domain/type combinations, Unix datagram support, privilege
  checks, and aligned/unaligned bad result pointers passed.
- `socketpair02`: `SOCK_CLOEXEC` and `SOCK_NONBLOCK` descriptor state passed.
- `recv01`: invalid fd/socket/buffer and unsupported message flags passed.
- `recvfrom01`: the recv checks plus invalid source-address length passed.
- `recvmsg01`: invalid message/iovec/control inputs, permission reception, and
  oversized control-message handling passed.

## Verification

- Kernel: RISC-V64 LTP glibc-only build at commit `aa0ef279`.
- QEMU: `virt`, one CPU, 1 GiB RAM, OpenSBI, disposable qcow2 overlay.
- Limits: 15 seconds per case and 90 seconds for the QEMU session.
- Runtime: approximately 1.4 seconds for the guest workload.
- Log: `/tmp/wateros-socket-read-20260801.log`.

No TFAIL, TBROK, timeout, panic, fatal trap, or fd restoration warning was
observed. Data-plane SMP stress and virtio-network throughput remain separate
nighttime gates.
