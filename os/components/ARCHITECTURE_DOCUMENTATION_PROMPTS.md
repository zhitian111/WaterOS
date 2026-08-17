# WaterOS Components Architecture Documentation Prompts

This pack has one dispatchable prompt for each of the 17 first-level components
in `os/components/`.  For an assignment, send the **common contract** together
with exactly one component prompt.  The intended deliverable is that
component's `README.md`; create it when it does not yet exist.

## Dispatch and Completion

The component inventory is intentionally explicit. A completed documentation
pass has one reviewed README for every row below; do not silently skip a
component because it has no top-level facade or because its current README is
already non-empty.

| Component | README at pack creation | Prompt section |
| --- | --- | --- |
| `wateros-base` | exists | [`wateros-base`](#wateros-base) |
| `wateros-cred` | exists | [`wateros-cred`](#wateros-cred) |
| `wateros-debug` | exists | [`wateros-debug`](#wateros-debug) |
| `wateros-driver` | create | [`wateros-driver`](#wateros-driver) |
| `wateros-fs` | exists | [`wateros-fs`](#wateros-fs) |
| `wateros-gui` | exists | [`wateros-gui`](#wateros-gui) |
| `wateros-ipc` | create | [`wateros-ipc`](#wateros-ipc) |
| `wateros-klog` | create | [`wateros-klog`](#wateros-klog) |
| `wateros-mm` | exists | [`wateros-mm`](#wateros-mm) |
| `wateros-network` | exists | [`wateros-network`](#wateros-network) |
| `wateros-platform` | exists | [`wateros-platform`](#wateros-platform) |
| `wateros-runtime` | exists | [`wateros-runtime`](#wateros-runtime) |
| `wateros-syscall` | exists | [`wateros-syscall`](#wateros-syscall) |
| `wateros-task` | create | [`wateros-task`](#wateros-task) |
| `wateros-tty` | exists | [`wateros-tty`](#wateros-tty) |
| `wateros-utils` | exists | [`wateros-utils`](#wateros-utils) |
| `wateros-vfs` | exists | [`wateros-vfs`](#wateros-vfs) |

Use one worker per row only when its edit is isolated to that row's README.
Cross-component facts may be inspected, but the assigned README is the only
file that worker changes. A coordinator should resolve contradictory claims at
the actual state owner rather than asking adjacent component documents to
paper over the disagreement.

Before marking a row complete, verify all of the following:

- Its README is Chinese, describes present source behavior, and preserves
  useful pre-existing material rather than replacing it wholesale.
- Every required information-architecture section is represented by a useful
  heading or a clearly equivalent subsection; consequential assertions name
  source paths and symbols.
- It has at least two source-grounded end-to-end diagrams or flows, except
  `wateros-utils`, whose narrower prompt deliberately permits a compact
  single construction/rendering path.
- Mermaid blocks render as text in Markdown and do not assert unimplemented
  behavior. Tables remain readable in a plain Markdown renderer.
- `git diff --check` passes for the README. Check relative Markdown links
  introduced or changed by the assignment; run the smallest relevant crate
  check when the documentation task also exposes a source or feature mismatch.

The final handoff for each row records the README path, the source areas
inspected, the verification commands and results, and any source-supported
limitation that could not be resolved from the available configuration.

## Common Contract (attach to every assignment)

```text
You are documenting one WaterOS kernel component.  Update only
`os/components/<component>/README.md` (create it when absent), preserving
correct useful material already present.  The document must describe the
current code, not an intended design.

First read the component's Cargo manifests, its facade `src/`, every active
implementation selected by the relevant features, and direct callers or
callees needed to establish the important paths.  Start with CodeGraph if the
repository has a `.codegraph/` index; use `rg` only when the index cannot
answer the question.  Check `git status --short` before editing and preserve
unrelated work.

Write Chinese technical documentation with this information architecture:

1. **定位和边界**: what the component owns, what it deliberately does not own,
   upstream/downstream dependencies, and the feature/architecture boundary.
2. **代码地图**: a compact table mapping semantic responsibilities to actual
   directories/files.  Explain facade/API/implementation layering, but do not
   turn this into an interface inventory.
3. **核心状态与数据结构**: a table for each consequential structure or global
   state: fields with semantic meaning, owner, allocation/storage form,
   sharing/locking or atomic publication rule, creation/destruction points,
   and key invariants.  Include bounded queues, caches, registries, side
   tables, page/descriptor pools, and hardware-facing state where applicable.
4. **关键链路**: give at least two real end-to-end paths as Mermaid sequence or
   flow diagrams plus accompanying prose.  Name actual functions/modules and
   include transitions, ownership transfer, waits/wakeups, error conversion,
   and cleanup.  Choose the paths that explain the component's primary
   runtime behavior rather than an API call list.
5. **机制与正确性**: explain state machines, lock ordering, atomic memory
   ordering, interrupt/process context boundaries, blocking rules, cache or
   consistency rules, resource lifetime, and failure handling that the code
   actually relies on.  State explicitly when an expected mechanism is not
   implemented yet.
6. **初始化、配置与可观测性**: initialization/preconditions, feature selection,
   RISC-V versus LoongArch differences when relevant, capacity and unit
   constants, logs/debug hooks, and verification entry points.
7. **限制与后续边界**: only current, source-supported limitations; do not call
   planned behavior supported or verified.

The document's center of gravity must be mechanisms and data flow.  Do not
paste trait signatures, enumerate every syscall/method, repeat generic Rust
concepts, claim Linux compatibility without evidence, or use vague wording
such as "the module handles management".  Every important assertion should
be traceable to a source path and symbol.  Keep examples short; use diagrams
only when they clarify a real relationship.  Preserve relative links and run
`git diff --check` after the edit.
```

## `wateros-base`

```text
Target: `os/components/wateros-base/README.md`.

Document the minimum dependency floor and explain why it must not depend on
platform, task, MM, or syscall.  Cover `CpuId`, `CpuMask`, and `CpuLocal<T, N>`
as capacity-bounded per-CPU data structures, including logical CPU numbering,
mask invariants, indexing rules, and cross-CPU access constraints.  Explain
the distinct publication/lifetime semantics of `MultiprocessorSafeCell`,
`BootOnceCell`, and `RuntimeOnceCell`; identify their backing synchronization
primitives and bootstrap-to-runtime transition.  Treat `base-config` as part
of this component: map its configuration domains to consumers, distinguish
compile-time capacities from discovered runtime state, record units and
fallback semantics.  Trace a CPU-local initialization/use path and a one-time
publication path from an actual caller.
```

## `wateros-cred`

```text
Target: `os/components/wateros-cred/README.md`.

Focus on the per-task credential side table rather than the credential trait
surface.  Document `ProcessCredentials`, `PerTaskCredRegistry`, owner and
reference-count bookkeeping, key choice, locks, and the invariants that make
thread sharing and process copying safe.  Trace user-task creation, fork,
`CLONE_THREAD` sharing, exec, and exit/reap cleanup from their actual hook
call sites, identifying who owns each transition.  Explain the initial-root
policy, the currently implemented `set*id` privilege rules, supplementary
groups, capability placeholders, and any deliberate gaps such as setuid-exec
handling.  State the exact interaction boundary with task, syscall, and VFS
permission checks and document races or missing lifecycle guarantees honestly.
```

## `wateros-debug`

```text
Target: `os/components/wateros-debug/README.md`.

Document this as a low-level GDB-facing diagnostic ABI, not as a general
logging system.  Analyze the per-CPU snapshot slots, active-slot publication,
event ring record layout, sequence protocol, counters, build ID exports, and
`TrackedMutex`/lock-recording mechanism.  Explain exactly how readers can
observe a partially updated CPU and why the Release/Acquire or sequence
validation protocol remains usable.  Trace an instrumented scheduler/lock
event from recording to host-side inspection and cover feature-disabled
behavior, `gdb-debug`, fault-injection separation, and the no-dependency rule
that prevents debug instrumentation from creating kernel lock cycles.
```

## `wateros-driver`

```text
Target: `os/components/wateros-driver/README.md` (create it if absent).

Treat driver as the device-discovery and device-I/O aggregation layer.  Map
the generic driver API, QEMU board assembly, and block/character/network/
display/input subcomponents.  Identify the real device registries, shared
device handles, probe order, DTB/MMIO/PCI discovery inputs, and lifecycle of
registered devices.  Follow at least: (1) boot-time QEMU device discovery to
a consumer obtaining a device, and (2) a real data request through a VirtIO
transport/queue to completion, including DMA/buffer ownership and interrupt
or polling handoff.  Describe block-cache layering separately from the raw
block driver, explain MMIO versus PCI and RISC-V versus LoongArch profile
differences, and identify where character/display/input/network mechanism
ownership begins and ends.  Do not mistake API traits for the driver runtime.
```

## `wateros-fs`

```text
Target: `os/components/wateros-fs/README.md`.

Explain the filesystem implementation selection and root-volume lifecycle.
Cover the static `FsImpl` registration/selection mechanism, `FsKind` and
access-mode selection, active root state, root device path, mount generation,
and shared read-only/read-write handles.  Trace boot `init` through devfs
refresh, root device discovery, implementation injection, and the later
default root mount; distinguish this sharply from VFS mounting.  Analyze how
devfs, procfs, rootfs, ramfs, and the selectable ext4 adapters participate,
including the default another-ext4 adapter's metadata/inode/lookup-cache and
block-I/O conversion mechanisms.  Record backend feature exclusivity,
write/sync boundaries, error translation, lock ownership, and what is vendor
code versus WaterOS adaptation.  Include a read/write persistence path without
claiming VFS semantics that are actually owned by `wateros-vfs`.
```

## `wateros-gui`

```text
Target: `os/components/wateros-gui/README.md`.

Document the kernel GUI runtime as a software compositor.  Center the text on
`GuiRuntime`, `ShadowSurface`, `DirtyRegions`, desktop/window z-order and
focus state, widget/input/output queues, and the input bridge.  Explain pixel
layout, stride validation, bounded dirty-region merge/fallback behavior,
double-buffer ownership, frame commit, and the fixed GUI-runtime-to-display
lock order.  Trace raw VirtIO input to semantic GUI events and a state-changing
event to damaged-region composition and framebuffer flush.  Cover hit testing,
dragging, text input, focus transitions, queue overflow behavior, rendering
primitives, device-format constraints, feature selection, and the explicit
boundary to display/input drivers and `user-graphics` ownership.  State
current limitations (kernel-only protocol, ASCII/US input, flush behavior)
precisely.
```

## `wateros-ipc`

```text
Target: `os/components/wateros-ipc/README.md` (create it if absent).

Explain each IPC mechanism by the state it owns and its task-lifecycle links:
pipe ring buffers and endpoint refcounts; waitqueue waiter registration and
wake protocol; futex keys, waiter registry, timeout and robust-list cleanup;
signal pending/mask/handler/timer state and delivery; eventfd counters; and
shared-memory segment/frame ownership and attachment mappings.  Map the
facade and all `ipc-*` subcrates, but avoid a syscall catalog.  Trace at least
a blocking pipe or futex operation from syscall/task context through sleep and
wakeup, and a signal or process-exit cleanup path.  Make lock order, lost
wakeup avoidance, atomic/queue invariants, interruption/timeout handling,
fork/clone/exec/exit ownership rules, and known unsupported Linux semantics
explicit.  Read the futex and signal local readmes before judging behavior.
```

## `wateros-klog`

```text
Target: `os/components/wateros-klog/README.md` (create it if absent).

Document the kernel log buffer as a concurrent observability data structure.
Find and explain the message-slot ring, payload/text ring or buffers, sequence
numbers, cursors, overwriting policy, record metadata, capacity constants,
and synchronization/atomic protocol.  Trace a kernel log emission through
formatting/storage to a reader (for example procfs/syscall/debug consumer),
including truncation, wraparound, filtering, and reader-overrun behavior.
Describe initialization and the boundary to `wateros-runtime` console logging:
which output is immediate and which is retained.  Establish whether logging
is interrupt-safe, allocation-free, SMP-safe, and non-blocking from code;
never infer those properties merely from its purpose.
```

## `wateros-mm`

```text
Target: `os/components/wateros-mm/README.md`.

Write a mechanism-first account of physical-frame and virtual-address-space
management.  Cover physical page/frame allocator state and `OwnedPhysPage`
lifetime, address/page newtypes, kernel mapping assumptions, page-table node
ownership, user address-space state, region/VMA bookkeeping, permissions,
and TLB/ASID behavior.  Compare Sv39 and LoongArch64 implementations only at
their actual divergence points.  Trace (1) ELF/user stack construction into a
new task and first address-space activation, and (2) mmap/brk/page-fault or
copy-to/from-user through page lookup/allocation/permission checks and
unmapping.  Explain page alignment, half-open range and overflow rules,
allocation/reclamation, fault error paths, zero-fill/COW/shared-map status,
inter-CPU shootdown requirements, and the boundary to task/platform.  Keep
trait definitions brief and make unsupported mechanisms unmistakable.
```

## `wateros-network`

```text
Target: `os/components/wateros-network/README.md`.

Document the protocol-stack runtime rather than socket API types.  Analyze the
global smoltcp stack state, NIC adapter, socket pools/handles, TCP/UDP state
tracking, route/interface configuration, polling/timer ownership, and the
VFS socket-handle bridge including receive leases/reservations.  Trace packet
arrival from a network driver through poll into a blocked socket read, and
trace TCP connect/listen/accept or transmit from syscall/VFS bridge to NIC
egress.  Explain how socket state snapshots relate to actual backend state,
who wakes waiters, nonblocking and error behavior, buffer ownership, close
cleanup, lock rules, and what AF_UNIX remains owned by syscall instead.  State
feature dependencies and the separation between generic socket semantics,
smoltcp, driver transport, VFS fd tables, and the top-level poller task.
```

## `wateros-platform`

```text
Target: `os/components/wateros-platform/README.md`.

Explain the hardware-environment boundary with a clear split between ISA arch
code and QEMU board/firmware profile code.  Cover boot/early-init state, DTB
physical-pointer handling and memory discovery, CPU/hart mapping and online
state, timers/timebase, IPI send versus local-pending acknowledge, reset,
console routing, trap/context primitives, and page-table activation hooks.
Trace BSP boot and AP bring-up through the actual RISC-V OpenSBI and
LoongArch64 paths, and trace timer/IPI delivery through the component boundary
to task scheduling.  Explain per-architecture data structures, assembly/Rust
layout contracts, initialization ordering, capability errors/fallbacks, and
the exact ownership boundary with drivers, MM, task, and runtime.  Do not
flatten board-specific mechanisms into a fictional common implementation.
```

## `wateros-runtime`

```text
Target: `os/components/wateros-runtime/README.md`.

Document runtime composition and boot-time failure behavior: console/serial
output path, global `log` logger, panic handling, platform shutdown or halt,
and global heap allocator.  Analyze the concrete allocator backend(s), heap
region provisioning, metadata/state, allocation and OOM behavior, statistics,
and the point at which allocation becomes legal.  Trace normal formatted
logging from a caller to UART/console and a panic from `panic_handler` through
best-effort output and termination.  Establish exact initialization ordering
and recursion/locking constraints, compile-time log-level feature selection,
early-boot behavior before full platform services, and whether each subsystem
is a facade or owns runtime state.  Separate retained kernel logs from direct
console output.
```

## `wateros-syscall`

```text
Target: `os/components/wateros-syscall/README.md`.

Treat this as the user/kernel transaction and dispatch layer.  Explain the
trap-to-dispatch handoff, syscall-number decoding, dense function-pointer
table/unsupported dispatch behavior, argument register packaging, `UserRet`
and `-errno` conversion, and restart/signal return handling.  Document the
user-memory copying and fallible-buffer mechanisms in terms of validation,
cross-page access, partial success, and fault propagation.  Use representative
end-to-end paths from distinct classes: one filesystem operation into VFS/FS,
one process or memory operation into task/MM, and one blocking
socket/poll/IPC operation through wait/wakeup.  Cover special in-component
state such as epoll/poll objects and AF_UNIX registries without assigning
network/VFS/task ownership to syscall.  State unknown-flag, overflow,
alignment, signal interruption, cleanup, and error-mapping rules based on
code, not a list of every syscall.
```

## `wateros-task`

```text
Target: `os/components/wateros-task/README.md` (create it if absent).

Document the task/process core and scheduler as state machines.  Analyze TCB,
process/thread-group records, kernel/user stacks, trap-frame/context state,
PID/TID and registry structures, parent-child/zombie bookkeeping, per-CPU
runqueues, scheduler classes and priority/vruntime data, wait queues, and
CPU-affinity/online-state interactions.  Trace (1) user task creation/ELF
handoff to first schedule/return-to-user, (2) fork/clone/exec resource
sharing/copying, and (3) block-wakeup-exit-wait/reap including signal or timer
interaction where the code shows it.  Explain every meaningful task-state
transition, locks and ordering, lock-free/atomic use, context-switch boundary,
SMP migration/IPI rules, and cleanup hooks into MM/VFS/IPC/cred.  Do not
substitute a trait summary for the lifecycle analysis, and label incomplete
Linux process semantics accurately.
```

## `wateros-tty`

```text
Target: `os/components/wateros-tty/README.md`.

Focus on terminal state and line discipline.  Document terminal and PTY pair
objects, master/slave endpoints, input/edit buffers, readable/output queues,
termios flags, foreground process-group/session state, pending control events,
and wait/wakeup state.  Trace UART/character-device bytes through VFS
adaptation into `feed_input`, canonical/raw transformation, echo, blocking
read, and signal delivery; also trace a PTY master-to-slave or slave-to-master
transfer and close/hangup.  Explain the exact rules for CR/LF conversion,
line editing, EOF, `VMIN`/`VTIME` if implemented, foreground signals, output
postprocessing, queue capacity, nonblocking behavior, lock-release-before-
callback rules, and lifecycle cleanup.  Clearly separate TTY mechanics from
VFS path/fd ownership and syscall ABI conversion.
```

## `wateros-utils`

```text
Target: `os/components/wateros-utils/README.md`.

Document this deliberately small pure-utility boundary.  Explain the facade
and `table-format` implementation's data layout (`FixedTable`, columns,
cells, alignment, writer), fixed versus automatic width calculation, and the
no-allocation/no-I/O constraints.  Trace a table's construction through
measurement and rendering into a caller-provided formatter, covering input
validation, Unicode/newline assumptions actually present in code, truncation
or overflow behavior, and determinism.  State why this crate must not acquire
kernel-global state or depend on task/MM/platform.  Keep the document compact
but still evidence-based; do not invent an architecture that the component
does not have.
```

## `wateros-vfs`

```text
Target: `os/components/wateros-vfs/README.md`.

Document VFS as the owner of per-task fd sessions, paths, mount namespace, and
page-cache/file-handle coherence.  Analyze fd-table and cwd state, open-file
and offset ownership, path normalization/resolution and symlink rules, mount
entries/identities/propagation/bind routes, backend routing, device mapping,
and page-cache file entries/dirty-page/writeback state.  Trace at least: (1)
`openat` from syscall through cwd/path/mount resolution to a filesystem handle,
(2) a cached read/write through reservation/commit or writeback and close/fsync,
and (3) fork/exec/exit treatment of descriptors and cwd.  Explain read leases
and user-copy commit semantics, FD sharing/dup/close lifetime, cache
invalidation on mount changes, precise lock ordering, blocking restrictions,
errors, and the boundary to FS, drivers, task, GUI and syscall.  Include
temporary files, proc/dev mounts, and framebuffer mappings only insofar as the
source implements them.
```
