# IRQ-driven VirtIO block integration archive

## Status

- Archive branch: `feat/irq-block-io`
- Base commit: `ce9bc894` (`[docs] record virtio direct descriptor experiment`)
- Target platform: QEMU RISC-V64/OpenSBI with VirtIO MMIO block devices
- Current conclusion: the interrupt infrastructure is functional, but the block I/O path is
  not yet truly asynchronous and has not demonstrated a BuildStorm performance improvement.

This branch is intentionally retained as infrastructure for a future asynchronous block-I/O
iteration. It should not be merged solely as a BuildStorm optimization.

## Implemented infrastructure

- Added the `wateros-irq` component as a WaterOS-facing wrapper around `irq-framework 0.3.2`.
- Added RISC-V PLIC external-interrupt operations, MMIO mapping, supervisor external-interrupt
  control, trap dispatch, and BSP/AP PLIC context initialization.
- Added IRQ registration for the QEMU VirtIO MMIO block device.
- Added `from_mmio_with_irq`, `read_blocks_nb`, and `write_blocks_nb` to the VirtIO block driver.
- The VirtIO ISR reads and acknowledges the MMIO interrupt status and advances a completion
  generation counter.
- Added a scheduler CPU snapshot flag so early boot and idle contexts continue to use the
  existing synchronous polling path.
- Routes the block IRQ to the BSP PLIC context once during registration, avoiding per-request
  PLIC affinity writes.
- Kept LoongArch64 isolated from the RISC-V-specific implementation through feature gating.

## Current I/O behavior

The runtime-capable path submits a nonblocking VirtIO request and then waits in the caller's
current interrupt state for either the IRQ completion generation to advance or the used-ring
token to become visible. The used-ring check remains as a compatibility fallback.

This is an interrupt-capable foundation, not a complete asynchronous design: requests are still
effectively serialized, callers do not yield while waiting, and there is no multi-request queue
or completion-to-task wakeup path. Consequently, it primarily removes part of the device-status
polling behavior rather than overlapping storage latency with useful work.

## Experiments and constraints discovered

- Sleeping from the early boot stack is invalid; the new boot-context check keeps that path on
  synchronous polling.
- Each online hart needs its own PLIC supervisor context initialized before accepting external
  interrupts.
- Attempts to sleep on a wait queue, replace the device lock with a sleeping mutex, or enable
  nested interrupts/WFI exposed deterministic SMP deadlocks. The ext4/VFS call chain can retain
  legacy spin locks across block I/O, so scheduling or changing interrupt state inside the block
  driver is not currently safe.
- A per-request IRQ-affinity update added avoidable PLIC MMIO work. The archived implementation
  instead fixes the device IRQ to the BSP context during registration.

Experimental diagnostics and unsafe/deadlocking wait variants were removed before archival.

## Verification record

- `make rv_check`: passed.
- `make la_check`: passed.
- Final QEMU SMP=8 functional run: CAgent passed 10/10 cases.
- The same functional run passed BuildStorm toolchain, minibuild, and compile phases.
- Trusted isolated IRQ BuildStorm compile sample: `962.23 s`. The user confirmed that only this
  IRQ QEMU workload was running, so this is the retained reference result.
- Raw log from a separate functional pass: `/tmp/wateros-irq-final-passed.log`. Its BuildStorm
  compile time (`1052.45 s`) is not performance evidence because another QEMU competed for CPU.

BuildStorm is predominantly compilation work and therefore mainly CPU-bound in this setup. The
available result does not establish a meaningful speedup from interrupt-driven block completion.

## Future continuation

If a workload with meaningful storage wait time justifies continuing this work:

1. Introduce an asynchronous block contract with explicit submit/completion APIs.
2. Support multiple outstanding requests and map completion tokens to blocked tasks.
3. Audit and shorten, split, or replace ext4/VFS spin-lock regions that currently span I/O.
4. Wake tasks from deferred interrupt context rather than scheduling directly in the ISR.
5. Compare polling and IRQ variants with fixed CPU allocation and no competing QEMU processes,
   using both elapsed time and CPU utilization/wait metrics.

The branch can later be merged normally or its archive commit can be cherry-picked when this work
becomes relevant again.
