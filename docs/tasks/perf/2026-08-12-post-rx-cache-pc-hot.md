# Post-RX-cache BuildStorm PC-hot resampling

## Purpose

The accepted private RX mmap cache and its 128 MiB capacity reduced BuildStorm
from 783.00 s to 534.26 s. That 31.77% cumulative change invalidates older
300-second hotspot rankings: repeated executable page faults, VFS copies, block
I/O, allocator work, and VirtIO waits should all have shifted.

Recent cache diagnostics still show a scan-heavy file page cache, but earlier
active/inactive replacement and direct-fill experiments did not improve wall
time. The executable-file rodata extension also regressed to 548.33 s. A fresh
instruction profile is required before selecting another implementation.

## Method

Run the accepted main RISC-V Final kernel for a fixed 300 s with only the
`pc-hot` QEMU plugin. This is diagnostics, not a wall-clock score. Keep the
fixed image, runner, CPU affinity, and snapshot setup; run no concurrent QEMU or
build. The expected runner timeout before the compile marker is not a failure if
toolchain/minibuild progress and panic/stall checks remain healthy.

After completion, read `result.json` and aggregate the plugin output against the
exact kernel ELF. Use the new top symbols and total instruction count to choose
one structurally new, bounded optimization. Do not revive rejected experiments
solely because an old symbol remains visible.
