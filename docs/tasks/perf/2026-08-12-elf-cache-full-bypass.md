# ELF readonly page-cache full-capacity bypass

## Evidence and problem

The accepted main BuildStorm reference is 534.26 s. The readonly exec ELF cache
has a 16,384-page (64 MiB) cap and a high reuse rate. Fixed-window diagnostics
on the current workload reached 14,362 residents after 81,920 lookups at about
300 seconds:

```text
lookups=81920 hit=67385 miss=14535 installs=14362 duplicate_load=172
evict=0 resident=14362
```

At capacity, the current miss path selects an LRU victim with:

```rust
entries.iter().min_by_key(|(_, entry)| entry.last_used)
```

That is an O(16,384) BTree traversal for every subsequent miss. Full BuildStorm
runs for about another 230 seconds, so the resident growth trend is likely to
cross the cap during the hottest compiler/linker interval. The scan also reads
`last_used` from every entry, creating cache pressure even though only one
victim is needed.

## Candidate

Match the already accepted private RX mmap cache admission rule:

1. Preserve the existing 16,384 entries and every hit/refcount/version rule.
2. When full, do not scan or evict. Load the missing page exactly as today but
   return its initial frame reference only to the current mapping.
3. Keep the established hot set stable. New one-shot pages bypass cache
   admission and disappear when their mapping exits.
4. Add a diagnostics `full_bypasses` counter in place of `evictions`; ordinary
   Final builds contain no counter operations.
5. Do not change VFS, mmap cache, ELF key, page size, capacity, or page-table
   behavior.

The tradeoff is deliberate: a page first seen after saturation cannot displace
an older entry and therefore cannot become a cross-process cache hit. In
exchange, a miss remains O(log n) for lookup plus I/O instead of O(n) victim
selection. The current cache's high cumulative hit rate and late saturation
make preserving the established set the lower-risk policy.

## Verification and acceptance

Run `make check`, `make la_check`, and `make all`; verify both artifacts and
`SCRIPT_BODY_FLAT_BEGIN`. Then run one fixed-image RISC-V BuildStorm sample.
Accept a first complete result below 524.26 s; reject a regression or
noise-sized result without a repeat. Only an accepted candidate may enter main.

## Result: rejected after adjacent main calibration

The candidate's first run passed all required markers and judge checks in
563.59 s, with no panic/SIGSEGV/stall/timeout. Because earlier candidates in
the same session had drifted from 546 s to 563 s, an immediately adjacent main
run was required instead of comparing only with the historical 534.26 s:

| run | guest compile time |
| --- | ---: |
| ELF full-cache bypass (`4736be2d`) | 563.59 s |
| current main (`641e9e27`) | 553.13 s |
| candidate delta | +10.46 s / +1.89% |

Both runs used the fixed image SHA-256
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The candidate kernel SHA-256 was
`a03109432ffde516f29f6739189047031eed8c11b02898275d860f41370e5fcb`.

The candidate is rejected and does not enter main. The result implies that
late-arriving ELF pages have enough reuse that replacing older entries is
worthwhile despite the O(16K) victim scan. A future change should preserve
replacement while making it O(1), for example with an intrusive LRU keyed by
stable entry indices; it should not freeze the initial hot set.
