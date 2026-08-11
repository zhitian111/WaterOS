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

## Result and next decision

The fixed window ended at the expected 300 s timeout after toolchain and
minibuild passed. There was no stall, panic, or SIGSEGV. The plugin counted
42,772,182,521 guest instructions. The leading resolved kernel symbols were:

| symbol / family | instructions |
| --- | ---: |
| compiler-builtins `memcpy` | 4,014,113,778 |
| `memcmp` | 2,405,622,881 |
| `memset` | 2,018,170,751 |
| TLSF allocate | 1,500,081,463 |
| VirtIO `add_notify_wait_pop` | 1,296,160,914 |
| TLSF deallocate | 1,058,835,823 |
| `normalize_absolute_path` | 877,460,433 |
| allocator guard alloc/dealloc/realloc | 1,650,800,655 combined |

The current profile therefore still points to copying plus allocation count,
not only lock contention. One previously untested shared source is
`another_ext4::split_path`: it constructs `Vec<String>` for each lookup/create
path and remove/rename then join the owned components again. Its split/map
iterator is 225,401,881 instructions; `str::join_generic_copy` is 67,598,322,
with `String`, RawVec, memcpy, and TLSF costs accounted separately above.

The structured runner result is
`/tmp/wateros-buildstorm-fixed/post-rx-cache-pchot-300s/result.json`; raw PCs are
`/tmp/wateros-buildstorm-fixed/post-rx-cache-pchot-300s/pc-hot.txt`. The next
bounded experiment will walk borrowed `&str` components in another-ext4 and use
`rsplit_once` for parent/name separation. It must not change directory parsing,
VFS caching, or on-disk behavior.
