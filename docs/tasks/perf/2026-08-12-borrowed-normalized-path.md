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

## Result (rejected)

The VFS API tests, `make check`, `make la_check`, and `make all` passed. Both
Final artifacts were produced and the RISC-V kernel retained the script-body
marker. The first and only fixed-image BuildStorm sample passed every required
marker, produced the expected 1,681,000-byte artifact, and had no panic,
SIGSEGV, stall, timeout, or filesystem error.

| item | result |
| --- | ---: |
| accepted main baseline | 534.26 s |
| borrowed normalized path | 569.55 s |
| regression | 35.29 s / 6.61% |
| host wall time | 592.716 s |

The candidate kernel SHA-256 was
`951e98d868a2e5209fcb2ab5bbc81a7378d1c7db5aadd08c9df9fcb5279953d0`;
the fixed image SHA-256 was
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/borrowed-normalized-path-a1/result.json`.

Removing the owning copy did not convert the symbol's instruction share into
wall-clock benefit. The lifetime-bearing `Cow` changes surrounding code shape
and the canonical-path scan remains; those costs, or workload variance, exceed
the eliminated allocation in this matched run. Because the result is a clear
regression, no plugin run or repeat is justified. Do not merge this candidate;
main remains the accepted 534.26 s kernel.
