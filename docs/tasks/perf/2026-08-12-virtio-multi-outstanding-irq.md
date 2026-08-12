# VirtIO multi-outstanding IRQ block-I/O experiment

## Context

The accepted main kernel completes the fixed-image RISC-V BuildStorm compile
in 534.26 s. A post-change profile attributes about 1.296 billion instructions
to VirtIO's synchronous `add_notify_wait_pop` path. BuildStorm is primarily a
parallel compiler workload, but every page-cache miss eventually performs a
synchronous 4 KiB ext4 block read and busy-polls the VirtIO used ring.

The archived `feat/irq-block-io` branch already provides a WaterOS wrapper for
`irq-framework 0.3.2`, RISC-V PLIC/trap integration, and VirtIO-MMIO interrupt
acknowledgement. Its trusted BuildStorm result was 962.23 s, so that version is
not an optimization candidate. Its request remained serialized by
`SharedBlockDevice = Arc<spin::Mutex<Box<dyn BlockDevice>>>`, callers did not
yield useful CPU time, and only one request could occupy the queue. Replacing
polling with interrupt delivery added overhead without creating overlap.

The current path contains three independent serialization regions:

```text
SharedRwFs spin lock
  -> another_ext4 global block-cache spin lock
    -> SharedBlockDevice spin lock
      -> VirtIO add_notify_wait_pop busy poll
```

An IRQ-only change cannot improve this path. This experiment therefore treats
IRQ completion and lock splitting as one indivisible candidate.

## Hypothesis and design

Rebase the archived `irq-framework` foundation onto current main, then make
the common RISC-V block-read path support several requests in flight:

1. Change the WaterOS block-device contract to shared `&self` operations and
   make `SharedBlockDevice` an `Arc<dyn BlockDevice>`. Each implementation owns
   only the synchronization it actually needs.
2. Split the write-through block cache into a short metadata/data-cache lock
   and an independently callable backend. Cache hits remain entirely under the
   short lock. On a miss, release the cache lock, read the backend, reacquire
   the lock, and install with a second check. Never sleep with the cache lock.
3. In the RISC-V VirtIO-MMIO driver, protect queue submission and used-ring
   retirement with a short queue lock. A request owns stable request, response,
   and data buffers while blocked; the queue lock is released before waiting.
   The IRQ handler only acknowledges MMIO status, advances a completion
   generation, and wakes waiters. Up to the 16-entry VirtQueue limit may be
   outstanding; the eight-core workload stays below it.
4. Completion order may differ from submission order. A waiter only retires
   its token when that token is at the used-ring head. After one retirement it
   wakes all waiters again so the owner of the next already-completed token
   cannot sleep waiting for an IRQ that has already occurred.
5. Permit concurrent read-side root-filesystem calls by changing `SharedRwFs`
   to a read/write lock and using read guards for the `&self` filesystem API.
   Mutating operations retain exclusive write guards. Release
   `another_ext4`'s block-cache lock around backend reads with a recheck on
   return. This is the minimum needed for multiple compiler tasks to reach the
   queue concurrently.
6. Preserve the synchronous polling path during boot, idle context, IRQ setup
   failure, and on LoongArch64/PCI. The public block API remains synchronous to
   callers; only its implementation blocks and yields while a request is in
   flight.

The root filesystem read guard can remain held while its owner sleeps. Other
readers may progress, but a concurrent writer attempting the exclusive guard
can spin until those reads complete. This is a bounded transitional risk, not
the final Linux-like locking model. The candidate is rejected immediately on
deadlock, starvation, data corruption, or a clear wall-clock regression.

## Implementation and verification

1. Restore the archived irq-framework/PLIC/trap foundation without its
   single-request polling behavior and build both architectures.
2. Refactor the block API, block cache, ext4 adapters, and both VirtIO drivers;
   add host tests proving concurrent miss recheck and cache coherence.
3. Convert `SharedRwFs` call sites to explicit read/write guards and run the
   narrow FS/VFS tests, followed by `make check` and `make all`.
4. Run a short RISC-V functional boot first because interrupt and scheduler
   races are not established by compilation. Read its serial log only if the
   command or required markers fail.
5. Run one fixed-image RISC-V BuildStorm sample and compare with 534.26 s.
6. If the first sample is a clear improvement beyond the roughly 10 s run
   noise, merge it to main without a repeat and run one focused post-change
   diagnostic. Otherwise reject it without a second performance run.

## Acceptance and stop conditions

All toolchain, minibuild, compile, artifact, and judge markers must pass with
no panic, stall, timeout, SIGSEGV, filesystem error, lost completion, or dirty
data loss. Both Final architecture artifacts must build and contain
`SCRIPT_BODY_FLAT_BEGIN`. A first successful sample below 524.26 s is accepted;
a result within the noise band or slower than main is rejected. The fixed
image is always used through QEMU snapshot mode.

## Result (rejected)

The complete candidate was implemented on `perf/virtio-multi-outstanding-irq`:

- the shared block-device API uses `&self` and implementation-local locking;
- the WaterOS block cache and `another_ext4` cache release their metadata locks
  around backend reads and recheck before installing data;
- the root read/write filesystem lock sleeps contended runtime tasks and
  permits concurrent readers;
- VirtIO-MMIO supports several nonblocking requests, IRQ wakeups, queue-full
  backpressure, used-ring-order retirement, and PLIC delivery on every online
  hart.

An initial smoke run found a concrete integration bug: a context rejected by
`irq::can_wait()` fell back to synchronous `add_notify_wait_pop` while other
asynchronous tokens were already present. That helper expects its own token at
the used-ring head and returned `WrongToken`. The fix kept all requests on the
nonblocking queue once IRQ mode was enabled; contexts that cannot sleep poll
the same queue instead of mixing completion APIs.

Verification after that fix:

- `make check`: pass;
- `make la_check`: pass;
- `make all`: pass, both Final kernels produced;
- standalone WaterOS block-cache tests: pass;
- `another_ext4 --features block_cache` tests: pass;
- 120 s fixed-image smoke: active until the intentional timeout, with no
  panic, SIGSEGV, block error, or runner stall.

The acceptance run
`/tmp/wateros-buildstorm-fixed/virtio-multi-irq-a6/result.json` used image SHA-256
`ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58` and timed
out at 900.023 host seconds. It never completed the first `cagent-glibc` command,
so no BuildStorm compile marker or guest compile time was produced. The main
kernel completes the full queue, including BuildStorm, in 534.26 s. This is a
hard rejection on functionality/progress as well as performance; a diagnostic
plugin run would only spend more time on an ineligible candidate and was not
performed.

The experiment establishes two constraints for any future IRQ attempt:

1. synchronous and asynchronous VirtQueue completion APIs must never share a
   queue while requests are outstanding;
2. converting the filesystem's outer lock into a sleeping reader/writer lock
   is not sufficient evidence that every nested legacy lock can safely survive
   a scheduler switch. The long `cagent` hang indicates a remaining lock-order
   or wakeup dependency in that broader conversion.

None of this candidate is merged into main. Main remains the accepted 534.26 s
kernel.
