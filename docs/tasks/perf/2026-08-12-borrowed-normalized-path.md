# Borrowed normalized-path fast path experiment

## Context

The accepted main kernel completes the fixed-image RISC-V BuildStorm compile
in 534.26 s. Its latest 300 s PC-hot sample attributes 877,460,433 guest
instructions to `normalize_absolute_path`, alongside 4.01B in `memcpy` and
2.56B in TLSF allocate/deallocate.

The current path code already detects canonical absolute paths with one
allocation-free byte scan. On the overwhelmingly common hit it nevertheless
constructs `String::from(path)`, allocating and copying the entire pathname.
Every VFS wrapper then borrows that new String only for the duration of the
call. This differs from Linux namei's basic rule that the pathname buffer is
borrowed while walking and ownership is introduced only when a longer-lived
object retains a component.

Earlier experiments optimized the normalization scan and changed
`another_ext4` component allocation. They did not remove this top-level owning
copy, so this is structurally distinct.

## Hypothesis and design

Give `NormalizedPath` a lifetime and store `Cow<'a, str>`:

1. if the input already satisfies the canonical absolute-path invariant,
   return `Cow::Borrowed(path)` with no heap allocation or pathname copy;
2. only malformed-but-normalizable inputs (`//`, `.`, `..`, trailing slash)
   execute the existing single-pass rewrite and return `Cow::Owned`;
3. retain the existing `as_str()` API so call sites that consume the value
   synchronously need no semantic change;
4. reject empty and relative paths exactly as before; preserve UTF-8 bytes and
   root-above `..` folding;
5. add tests that assert both semantic output and borrowed/owned representation.

The type must not escape the input lifetime. Call sites that need persistent
ownership already explicitly construct their own `String`; those copies remain
because they represent actual retained state.

## Verification

Run the VFS API unit tests, `make check`, `make la_check`, and `make all`, then
verify both Final artifacts and `SCRIPT_BODY_FLAT_BEGIN`. Run one fixed-image
RISC-V BuildStorm sample against main. Read build logs only on failure and do
not run a diagnostic plugin before wall-clock acceptance.

## Acceptance and stop conditions

Accept only if the first successful sample is below 524.26 s and all toolchain,
minibuild, compile, artifact, and judge markers pass without panic, stall,
timeout, SIGSEGV, or filesystem error. A clear win is sufficient and is not
repeated. Reject a noise-sized result or regression without a second run.
