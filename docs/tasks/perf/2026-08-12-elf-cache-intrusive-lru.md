# ELF readonly page cache intrusive O(1) LRU

## Evidence

The readonly ELF page cache holds at most 16,384 physical pages. A 300-second
diagnostic reached 14,362 residents, and full BuildStorm continues for roughly
another 230 seconds. Once full, every miss currently finds a victim by scanning
the entire BTree map for minimum `last_used`, making victim selection O(16K).

A bounded experiment that simply bypassed admission at capacity was slower
than an immediately adjacent main run:

| implementation | time |
| --- | ---: |
| keep old hot set, bypass late pages | 563.59 s |
| current main with replacement | 553.13 s |

Therefore late pages have meaningful reuse and replacement must remain. The
cost to remove is the full-map victim scan, not replacement itself.

## Design

Replace `BTreeMap<Key, Entry { ppn, last_used }>` with:

```text
BTreeMap<Key, slot_index>
fixed Vec<Slot { key, ppn, prev, next, identity }>
LRU head/tail slot indices
```

- Key lookup and insert remain O(log n).
- A hit detaches its slot and moves it to the LRU head in O(1).
- A miss below capacity appends one slot and links it at the head.
- A miss at capacity takes the tail slot in O(1), removes its old key from the
  BTree, replaces payload/identity/key, and moves the slot to the head.
- The evicted cache reference is released after dropping the cache lock.
- Concurrent duplicate loads, content-version retries, mapping references, and
  the 16,384-page memory bound remain unchanged.
- Diagnostics keep the same lookup/install/duplicate/eviction meanings.

No allocation occurs on hit or eviction after the slot Vec reaches capacity;
the map may still reuse its allocator nodes according to BTree implementation.
This experiment does not combine mmap/VFS caches or change admission scope.

## Verification and acceptance

Extend the directed MM cache test to force a small logical LRU ordering helper
where practical. Run `make check`, `make la_check`, and `make all`, verify both
kernel artifacts and `SCRIPT_BODY_FLAT_BEGIN`, then run one candidate followed
by an adjacent main only if host drift makes the historical 534.26 s comparison
ambiguous. Accept only a functional improvement exceeding about 10 seconds;
otherwise record and leave main unchanged.

## Result: rejected as noise-sized

The candidate passed all BuildStorm and judge markers with no panic, SIGSEGV,
stall, or timeout:

| run | guest compile time |
| --- | ---: |
| current main, immediately before candidate | 553.13 s |
| intrusive O(1) LRU (`cce74a09`) | 552.13 s |
| delta | -1.00 s / -0.18% |

Both used the fixed image SHA-256
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The one-second difference is far below the accepted noise margin, so the
candidate does not enter main and is not repeated. Together with the 10.46 s
regression from disabling replacement, this indicates that retaining late ELF
pages matters but victim selection itself is not a large wall-clock cost. The
next bounded experiment should test capacity headroom rather than further LRU
micro-optimization.
