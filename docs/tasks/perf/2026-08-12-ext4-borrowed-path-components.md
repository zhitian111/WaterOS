# another-ext4 borrowed path components experiment

## Evidence

After the accepted RX mmap cache improvements, the new 300 s PC-hot profile
counted 42.77 billion guest instructions. Copy and allocation remain dominant:
`memcpy` 4.01B, TLSF allocate/deallocate 2.56B, and allocator guards 1.65B.

`another_ext4::split_path` currently trims the leading slash and then builds a
`Vec<String>` by allocating and copying every component for every high-level
lookup/create/remove/rename. Remove and rename split that vector and join the
parent components into another allocated String before immediately parsing it
again. The current profile assigns 225.40M instructions to the split/map
iterator and 67.60M to `str::join_generic_copy`, apart from String, RawVec,
memcpy, and allocator symbols.

## Candidate

Keep the change intentionally confined to
`vendor/another_ext4/src/ext4/high_level.rs`:

1. Walk lookup components as borrowed `&str` slices directly from the caller's
   path.
2. Use a `Peekable` borrowed iterator in recursive create to detect the final
   component without materializing a vector.
3. Use `rsplit_once('/')` to borrow parent/name slices for remove and rename;
   do not allocate or join a parent path.
4. Preserve leading-slash trimming, root lookup behavior, empty components in
   non-root paths, lookup order, errors, and all on-disk operations.

This follows the Linux pathname-walk principle that path components are views
into the supplied pathname; ownership is introduced only when a dentry or other
long-lived object must retain a name. No VFS cache, directory parser, inode
cache, block I/O, or vendor on-disk structure is changed.

## Verification and acceptance

Add pure tests for root, leading slash, nested paths, duplicate/trailing slash,
and parent/name separation. Run the vendor crate tests, `make check`, and
`make all`; verify RV/LA Final aliases and script markers. Then run one matched
full RISC-V BuildStorm sample with the fixed image and runner.

The accepted baseline is 534.26 s. Accept only a successful result clearly more
than roughly 10 s faster, with no timeout, stall, panic, or SIGSEGV. A clear
first-run win is final and will not be repeated. A regression or noise-sized
change is rejected and documented without merging the implementation.
