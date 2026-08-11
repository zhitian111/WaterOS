# Executable-file readonly mmap cache experiment

## Evidence and scope

The accepted private RX mmap cache now completes BuildStorm in 534.26 s. Its
128 MiB diagnostics window reported 344,064 page lookups, a 91.97% hit rate,
27,585 resident pages, and no capacity bypass. This proves that pages belonging
to repeatedly mapped toolchain and shared-library files are a high-value set.

The rejected broad readonly mmap experiment (932.23 s) is not a reason to cache
all readonly files again. It admitted one-shot `.rmeta`, archives, and build
artifacts, originally performed an O(n) victim scan after filling, and caused
extra copies for mappings later made writable.

This experiment extends sharing only to non-writable private mappings whose
backing regular file has at least one Unix execute bit. That class includes the
readonly header/rodata segments of Cargo, rustc, linkers, executables, and most
shared libraries, while excluding ordinary source, `.rmeta`, and archive files.
Existing `PROT_EXEC` mappings remain eligible regardless of mode, preserving
the accepted behavior.

## Implementation

Reuse the metadata lookup already required by `sys_mmap` for file size; do not
add another path lookup or file read. Carry a boolean derived from
`metadata.mode & 0o111 != 0` into `VfsMmapPageLoader` admission:

```text
MAP_PRIVATE && !writable && (PROT_EXEC || backing file is executable)
```

The physical page cache key, 128 MiB capacity, content-version invalidation,
I/O-outside-lock behavior, full-cache bypass, and frame reference protocol are
unchanged. `MAP_SHARED`, writable private mappings, non-regular files, and
handles without stable content identity remain on the old path. If a mapping
later becomes writable, the existing page-table fault and COW checks must still
prevent installing a shared cache page writable.

## Verification and acceptance

1. Run `make check` and `make all`, then verify Final/default RV and LA aliases
   and `SCRIPT_BODY_FLAT_BEGIN` markers.
2. Run one matched full RISC-V BuildStorm sample using the fixed image/runner.
3. Require compile success and no timeout, stall, panic, or SIGSEGV.

The accepted baseline is 534.26 s. Accept only an improvement clearly larger
than the recent roughly 10 s system noise. A clear first-run win is final and
must not be repeated. A regression or noise-sized result is rejected and
documented without merging the implementation.
