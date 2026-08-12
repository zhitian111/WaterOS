# Online architecture CPU capacity

## Problem

The final runner starts WaterOS with 8 RISC-V vCPUs and 12 LoongArch vCPUs,
while the kernel used one architecture-independent static capacity of 32.
Actual LoongArch topology is already read from the QEMU DTB, but every
per-CPU scheduler, IPI, TLB, debug, and allocator array was unnecessarily
sized for 32 CPUs on both targets.

## Change

- RISC-V64 compile-time capacity: 8 CPUs.
- LoongArch64 compile-time capacity: 12 CPUs.
- Other host targets retain 32 slots for unit/configuration tests.
- Configured and online masks remain runtime platform state; the capacity does
  not pretend that every slot is present.

## Verification plan

1. Build both kernels from this branch.
2. Boot LoongArch with the final-runner-equivalent QEMU 9.2.1 command and
   confirm the 12-vCPU topology completes the recovered BuildStorm script.
3. Boot RISC-V with the corresponding 8-vCPU command and confirm the recovered
   BuildStorm script completes without the previous cargo/rustc stall.
4. Merge only after both functional runs produce a terminal
   `BUILDSTORM_RESULT` line.

## Delivery build invariant

`make all` is the final-delivery entry point. It builds the fixed release,
`final_online`, TLSF configurations directly and leaves exactly two top-level
kernel artifacts: `kernel-rv` and `kernel-la`. Intermediate `kernel-*-final`,
pre, log, and debug ELFs are neither dependencies nor permitted output of this
entry point. The recipe verifies the final filename set, ELF machines, and
copies against Cargo's two architecture artifacts.

## QEMU 9.2.1 RISC-V memory regression

The recovered online command passes `-m 16G`. Both QEMU 9.2.1 and QEMU 11
describe the complete RAM range in the DTB `/memory` node, but they place the
DTB blob at different physical addresses. QEMU 9.2.1 places it near 3 GiB,
whereas the tested QEMU 11 build places it near the top of RAM.

The old Sv39 initialization incorrectly used the DTB physical address as the
frame allocator's upper bound. Consequently WaterOS exposed only about 3 GiB
of the requested 16 GiB under the online QEMU 9.2.1. The parallel rustc build
eventually produced a user load-page fault, failed signal-frame setup, and
left Cargo apparently stalled.

The allocator now covers the complete DTB `/memory` range and excludes only
the pages occupied by the DTB blob. This is independent of QEMU version and
DTB placement.

## Verified result

- RISC-V, QEMU 9.2.1, 8 vCPUs, 16 GiB: `status=OK`, `elapsed_s=547.27`,
  `run=OK`.
- LoongArch, QEMU 9.2.1, 12 vCPUs, 36 GiB (UAL-capable main before this
  capacity-only change): `status=OK`, `elapsed_s=544.57`, `run=OK`.

The final candidate still requires one sequential run per architecture after
merging and rebuilding through the strict `make all` entry point.
