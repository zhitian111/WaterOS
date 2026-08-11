# K-01 Rename Replacement and Open-FD Identity Report

## Problem

`another_ext4::generic_rename()` rejects an existing destination with `EEXIST`.
WaterOS forwarded that limitation directly, although POSIX rename replaces a
compatible destination. The path-keyed page cache also left source handles on
the old path. After recreating that path, an fd opened before rename read the
new object instead of the renamed source.

Baseline evidence:

```text
mv: can't rename '/glibc/open-rename-old.txt': File exists
```

After adding replacement but before migrating source state, the paths and
replaced-target fd were correct, but the source fd returned `recreated-old`.

## Implementation

- VFS moves an existing destination to a unique same-volume temporary name,
  renames the source, restores the destination if that step fails, and removes
  the replaced temporary object after success. Vendor code remains unchanged.
- An open replaced regular file receives the same bounded detached snapshot
  used by unlink, so its old fds do not alias the new destination object.
- Source handles share a mutable backing path. Rename moves that state from the
  old registry key to the new key, so existing handles, duplicates, and staged
  reads continue against the renamed object.
- Page-cache entries for both path keys are dropped only after flush, while
  source open-reference counts move to the destination key. Registry, backing
  path, and ref-count publication is one non-I/O critical section.
- Renaming a non-directory over a directory now returns `EISDIR`.

## Verification

- Custom RISC-V guest test: existing-destination replacement passed; source old
  fd read `source-object`, replaced-target old fd read `target-object`, recreated
  old path read `recreated-old`, and the new path read source data plus a write
  issued through the old source fd.
- Existing LTP `rename09`, `renameat201`, and `renameat202`: 3/3 runner cases and
  8 TPASS results.
- Page-cache host tests: 13/13 passed, including source ref migration and target
  ref removal.
- `make check` and `make la_check` passed.
- The LTP overlay passed all five `e2fsck -fn` phases, exit code 0.

Logs: `/tmp/wateros-open-rename-baseline-20260801.log`,
`/tmp/wateros-open-rename-write-fix-20260801.log`,
`/tmp/wateros-rename-ltp-short-fix-20260801.log`, and
`/tmp/wateros-rename-ltp-short-e2fsck-20260801.log`.

## Remaining Limits

The fallback consists of multiple durable backend operations because
another-ext4 has no replace primitive; it cannot provide journal-atomic rename
across power loss. Full concurrent rename/write/fsync stress remains a K-01
nighttime gate. The existing 16 MiB detached cap also applies when replacing a
still-open destination. LTP rename01 through rename08 request a 300 MiB test
block device and currently hit the kernel heap-backed test-device limitation;
they were not counted as rename regressions.
