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

