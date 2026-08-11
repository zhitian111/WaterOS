# Direct current-process exit-state query experiment

## Context

The accepted main kernel completes fixed-image BuildStorm in 534.26 s. Its
post-RX-cache 300 s profile attributes about 355 million guest instructions to
`ProcessRegistry::process_task_snapshot`.

Every trap return to user space calls `exit_current_if_process_exiting`. The
current implementation obtains a full `ProcessTaskSnapshot` under the process
registry lock, releases it, then locks the registry again to obtain a full
`ProcessSnapshot`, only to compare `ProcessState` with `Exiting`. The common
not-exiting path therefore performs task-to-pid, process, and per-process task
BTree lookups plus snapshot construction, followed by another process lookup
and snapshot construction, on every syscall return.

## Hypothesis

Add a narrow registry query that maps the current scheduler task directly to
its process and returns only an `Exiting` code when present. It executes under
one existing registry critical section, does not walk the per-process task map,
and constructs no snapshot. Replace only the trap-return exit check; all APIs
that genuinely need task or process snapshots remain unchanged.

This is analogous to Linux hot paths reading a specific task/process state
field rather than materializing a diagnostic snapshot. Process state remains
authoritative in `ProcessRegistry`, and the same interrupt/lock guard preserves
synchronization with `exit_group` updates.

## Verification

1. Add a focused registry test covering running, exiting, and unknown tasks.
2. Run affected task tests where available, normal kernel check, and both
   architecture builds; inspect logs only on failure.
3. Verify default/Final hashes and the script-body marker.
4. Run one matched fixed-image RISC-V BuildStorm sample.

## Acceptance and stop conditions

Accept only if the first matched run passes all markers without timeout,
stall, panic, SIGSEGV, or exit-semantics regression and improves the 534.26 s
baseline by more than 10 s. A clear first-run win is sufficient and is not
repeated. Reject a regression or noise-sized change without a second
performance run.

## Result

The first and only matched sample passed all toolchain, minibuild, compile, and
judge markers. It produced the expected 1,681,000-byte artifact and exited
without timeout, stall, panic, or SIGSEGV.

| item | result |
| --- | ---: |
| accepted baseline | 534.26 s |
| direct exit-state query | 538.35 s |
| change | +4.09 s / +0.77% |
| host wall time | 560.893 s |

The candidate kernel SHA-256 was
`ef3c311128129d29e9f9b20462f770d03d32205a831ff6c3673264faeff21a39`;
the fixed image SHA-256 remained
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`.
The structured result is
`/tmp/wateros-buildstorm-fixed/process-exit-state-query-a1/result.json`.

The narrow query is semantically correct but the wall clock regressed by
4.09 s, so the profiled snapshot instructions were not a sufficient predictor
of BuildStorm completion time. Per the stop rule, do not run a second sample
and do not merge the implementation to main. Preserve this branch as the
performance-failed record. The standalone host unit-test command was also
blocked before this crate by the repository's existing no-architecture
`ArchPagingImpl` configuration; normal kernel check and both architecture
builds passed.
