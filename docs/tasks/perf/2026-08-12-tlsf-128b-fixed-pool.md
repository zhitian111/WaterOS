# TLSF 128-byte per-CPU fixed-pool experiment

## Why this experiment

The accepted main BuildStorm reference is 534.26 s. Its current 300 s PC-hot
profile still attributes about 1.50 billion instructions to TLSF allocate,
1.06 billion to TLSF deallocate, and another 1.65 billion to allocator guard
wrappers. Earlier low-overhead diagnostics during the compiler phase counted:

| size bucket | allocations |
| --- | ---: |
| 16 B | 5,580,621 |
| 32 B | 1,833,905 |
| 64 B | 2,382,653 |
| 128 B | 3,689,930 |

These four classes dominate allocation count, while measured TLSF lock
contention was only about 2.3%. The objective is therefore to bypass both the
TLSF algorithm and its shared mutex for the most common small allocations, not
to micro-optimize atomics inside the existing lock.

## Why the old slab result does not settle this design

The rejected `perf/tlsf-slab` candidate implemented eight classes up to 2 KiB,
16 KiB spans, per-span headers and optional bitmaps, central lists and locks,
per-class magazines, cross-CPU accounting, span drain, and reclamation. It
finished in 910.08 s against the then-main 880.44 s.

This experiment deliberately removes that machinery. It implements only one
physical slot size (128 B), serving layouts whose size and alignment fit that
slot. A bounded static pool is divided into CPU-owned chunks by one atomic bump
index. Each CPU keeps one intrusive local free list and a small current chunk.
No allocation header, bitmap, central free list, span scan, class lookup table,
or remote drain exists on the fast path.

## Design and ownership rules

1. Reserve a 16 MiB, 128-byte-aligned static pool outside the 128 MiB TLSF
   heap. This bounds the experiment at 131,072 objects (0.10% of guest RAM).
2. A layout is eligible when `0 < size <= 128` and `align <= 128`.
3. A CPU obtains 32 consecutive slots through one global atomic bump when its
   local list is empty, then serves the other 31 without shared writes.
4. Free stores the next pointer inside the object and pushes it onto the
   current CPU's local list. Cross-CPU free is valid: ownership transfers to
   the freeing CPU because no metadata is tied to the allocating CPU.
5. Pool addresses are recognized by a range and slot-alignment check before
   TLSF pointer validation. Pool storage is never returned to TLSF.
6. When the bounded pool is exhausted, new eligible allocations use TLSF.
   Existing pool objects remain valid and continue to recycle locally.
7. Realloc stays in place while the new layout still fits 128 B. Otherwise it
   allocates through the normal global path, copies `min(old,new)` bytes, and
   returns the old slot to the local pool.
8. The existing allocator interrupt guard remains around all operations, so a
   CPU-local free list cannot be interrupted or concurrently mutated.

The pool may retain freed objects on a CPU indefinitely; this is intentional
bounded caching, not a leak. The 16 MiB cap is smaller than the memory already
accepted for RX executable sharing and does not grow at runtime.

## Verification and acceptance

1. Add focused internal checks for layout eligibility, pool address
   classification, local reuse, and TLSF fallback behavior where practical.
2. Run `make check`, `make la_check`, and `make all`; confirm both kernel
   artifacts and `SCRIPT_BODY_FLAT_BEGIN`.
3. Run one fixed-image RISC-V BuildStorm sample against 534.26 s, with no
   concurrent QEMU. A first clear result below 524.26 s is accepted without a
   repeat. A regression or noise-sized result is rejected immediately.
4. Never merge the candidate to main before wall-clock acceptance. Record the
   result and preserve the experiment branch either way.

