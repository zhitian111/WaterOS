# LoongArch race-fix continuation log

This log records the continuation of `HOF-20260813-loongarch-race-fix`.
Commands are run from `/home/zhitian/project/WaterOS_refactor/os` unless noted.
Full serial/build logs are retained separately; this file records hashes, result
markers, failures, and decisions without copying the restricted test script body.

## Acceptance target

- LoongArch64: three consecutive full buildstorm runs from freshly decompressed
  images, using the repository test script variant that prints the nested QEMU
  output, with no abnormal result.
- RISC-V64: the same three-run acceptance.
- Prefer LoongArch-only fixes; touch common/RISC-V code only when evidence locates
  the race there.
- After final acceptance, remove decompressed final images and run
  `~/funny-script/syyu_and_shutdown.sh`.

## 2026-08-13 import and input audit

- State: `main@c413c4b97e36f91b1b4ba960209d00509f45ebd5`, no tracked changes.
- Relevant ancestors: `9bc17023`, `705950b4`, and `1f56057a` are present.
- Runtime: no active WaterOS QEMU/GDB, related mount, or loop device.
- Emulator: local LoongArch QEMU reports version 9.2.1.
- Clean compressed inputs:
  - LA gzip SHA-256: `2c411447274fbd83505d2fac505a5d9e8ed8ff3bdfc3d2d6cbdb8f61ff7d90d2`
  - RV gzip is to be hashed before RV acceptance begins.
- Authoritative unmodified test script:
  `/home/zhitian/Downloads/buildstorm_testcode.recovered.sh`, SHA-256
  `84d631012532e6817565cba02d35d8a2721c5ec7787a1e0519d6d0ae0a4274bb`.
- Acceptance input is a temporary byte-for-byte copy of that source with exactly
  one command appended at EOF: `cat /work/buildstorm.run.out`. The repository's
  separately modified diagnostic script is not used for acceptance.
- Space audit: only 15,003,328,512 bytes were free while one clean raw image is
  15,032,385,536 bytes. Per user instruction, old decompressed test images will
  be removed before recreating each clean round from `~/Downloads`.

## LoongArch attempt 1 — failed: all-vCPU-idle stall

- HEAD: `c413c4b97e36f91b1b4ba960209d00509f45ebd5`.
- Kernel SHA-256: `9cf57729421b3a19b81593a18ffb00bf075b7dffae382daebc9cec6f96c7eaa8`.
- Script input: recovered source plus only the EOF `cat` command; temporary
  SHA-256 `5b863ded09b8a35bd5af042ff71e6b670f9083de259cc6ae47cafeb89a79df61`.
- Image: freshly decompressed from the audited LA gzip, then script replaced;
  inode mode `0755`, size 7,578 bytes.
- QEMU: 9.2.1, 36 GiB, 12 vCPUs, online-equivalent devices.
- Observed: toolchain and minibuild reported `status=OK`; full build stopped
  after `Compiling hashbrown v0.17.1`. The serial log did not grow for four
  minutes. A two-sample host thread view showed all 14 QEMU threads sleeping;
  vCPU threads only consumed roughly 1–4% each from timer activity.
- Absent: no `BUILDSTORM_RESULT`, SIGSEGV, panic, or TLB-shootdown timeout.
- Stop: explicitly sent SIGTERM only to QEMU PID 748073 after preserving the
  failure signature. This run does not count toward acceptance.
- Serial log: `/tmp/wateros-la-acceptance-round1.log`, SHA-256
  `e595efa9c2b14657b35c46a4a19a5405f3588b64e5a154bac4c55386a38fdcd6`.
- Diagnostic limitation: this launch had no QMP/GDB endpoint and its stdin was
  closed. Host GDB attach was rejected by ptrace policy. The next clean attempt
  will expose QMP and a non-stopping GDB server from launch, then freeze only if
  the same stall is confirmed.

## LoongArch diagnostic attempt 2 — passed (consecutive acceptance 1/3)

- Same HEAD, kernel, source-plus-EOF-cat script, and freshly decompressed input
  recipe as attempt 1.
- Diagnostic-only launch endpoints: QMP Unix socket and non-stopping GDB server;
  both remained unused because the guest kept making progress.
- Result: `TOOLCHAIN_RESULT status=OK`, `MINIBUILD_RESULT status=OK`, and
  `BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=12 elapsed_s=519.88 ... run=OK`.
- EOF `cat` evidence: retained nested-QEMU output contains `Hello, world!`.
- WaterOS reported `all commands finished`; outer QEMU exited normally with 0.
- No SIGSEGV, panic, or shootdown timeout was found.
- Serial log: `/tmp/wateros-la-acceptance-round2.log`, SHA-256
  `e2f436fd0e78ac7a3271c442dee08279201d5ed360b60b41c1b352cd3f870bfe`.
- Acceptance accounting: 1/3 consecutive LoongArch passes. Attempt 1 remains a
  race failure and prevents claiming stability.

## LoongArch diagnostic attempt 3 — failed: all tasks blocked, all CPUs idle

- Same HEAD, kernel, source-plus-EOF-cat script, and fresh-image recipe as the
  previous attempts. QMP and GDB endpoints were enabled from launch.
- Observed: toolchain and minibuild passed. The full build advanced through the
  previous `hashbrown` point, then stopped after `Compiling uefi v0.37.0`.
  The serial log remained exactly 17,115 bytes with an unchanged mtime during a
  final 30-second confirmation window; all vCPU host threads showed only about
  1--5% timer activity.
- QMP `stop` froze the guest before collection. The old Loongson cross-GDB could
  not consume QEMU 9.2.1's 280-byte register packet (it expected 272 bytes), but
  host `gdb-multiarch` 17.2 collected all 12 CPU states and kernel globals.
- Guest state: all 12 CPUs were at `__wateros_idle_task_runtime_main`; current
  task IDs were the physical idle IDs 0--11 and all current address-space
  pointers were zero. The scheduler was ready at tick `0xfa83`; pending IPI
  bytes were all zero. For every CPU, LoongArch TLB pending and completed
  sequence values matched, including CPU 4's older `0x33ca2` value versus the
  global `0x33d3e`. Thus this captured stall is not a shootdown waiter: all user
  tasks are blocked and no runnable work remains.
- Absent: no `BUILDSTORM_RESULT`, SIGSEGV, panic, or shootdown timeout.
- Serial log: `/tmp/wateros-la-acceptance-round3.log`, SHA-256
  `c7a6f9ae648c82198c3b40e4c0225f972c81a0e4491a5fb867c9e37d07746f67`.
- GDB snapshot: `/tmp/wateros-la-round3-guest-gdb-multiarch.log`, SHA-256
  `e795a87c5400134892cf876fbaf4c8c860fbd67b479894d3159948441fd6d90b`.
  The incompatible cross-GDB transcript is separately retained at
  `/tmp/wateros-la-round3-guest-gdb.log`.
- QEMU PID 752131 was explicitly terminated after capture. This run does not
  count toward acceptance; the consecutive LoongArch pass count resets to 0/3.
- Next diagnostic: run a fresh image with the existing `stall-debug` feature so
  the watchdog records non-idle task wait targets and futex registry state.

## LoongArch diagnostic attempt 4 — passed, diagnostic kernel

- Built `kernel-la-final` with only the existing `stall-debug` feature added.
  Build exited 0; SHA-256
  `69184da9ada2c2b02600f03188d7e587faad4ecf8200040a45395bdae53efbcf`.
- Used another freshly decompressed image and the same audited source-plus-EOF-
  `cat` script.
- Result: toolchain/minibuild passed; full result was `status=OK`, `rc=0`,
  `elapsed_s=531.22`, `run=OK`; nested output contains `Hello, world!` and the
  outer guest completed normally. No stall snapshot, SIGSEGV, panic, or
  shootdown timeout appeared.
- Serial log: `/tmp/wateros-la-diagnostic-round4.log`, SHA-256
  `99a70f9d0d928939c65783edceb0bd66b43eb910f82970dfca2a8630fde6126f`.
- This run does not count toward acceptance because the kernel contains the
  diagnostic watchdog. Another clean diagnostic run is required to catch the
  intermittent all-tasks-blocked state.

## LoongArch diagnostic attempt 5 — passed, diagnostic kernel

- Same existing-watchdog diagnostic kernel and another freshly decompressed
  image. Full result: `status=OK`, `rc=0`, `elapsed_s=521.12`, `run=OK`, with
  nested `Hello, world!` and normal outer completion.
- Serial log: `/tmp/wateros-la-diagnostic-round5.log`, SHA-256
  `28ef260d89a136f6cbcc6cc85d22f0d05e2d40a271ea7eb4273160d743364037`.
- Two diagnostic passes after two failures in three ordinary-kernel samples
  suggest that the watchdog's periodically sleeping/waking kernel task may
  perturb the race. The next diagnostic kernel therefore disables that task on
  LoongArch and samples passively from CPU 0's existing timer interrupt. It
  emits only after 3,000 ticks with an unchanged syscall counter and only when
  every online CPU is idle with all runnable queues empty.

## LoongArch passive diagnostic attempt 6 — failed: likely stranded runnable work

- Passive diagnostic kernel SHA-256:
  `ff33a28c7b13da2b495ffcf88bcc5a76ff4f58e59204b6cc6ae1b7bb8a3369ae`;
  another freshly decompressed image used the audited script.
- Full build stopped after `Compiling axklib v0.7.5`. Host vCPU threads returned
  to only about 1--4% timer activity. GDB again found every CPU in the physical
  idle task, all current address-space pointers zero, no pending IPI, and every
  TLB pending/completed pair matched.
- The passive sampler did not emit because its first version additionally
  required every runnable queue to be empty. Since all CPUs were independently
  confirmed idle, this strongly indicates at least one runnable queue was
  nonempty while the scheduler nevertheless kept every CPU idle. The sampler
  is changed to trigger whenever all online CPUs are currently idle and will
  print both per-CPU runnable counts and every non-idle task state.
- Serial log: `/tmp/wateros-la-passive-round6.log`, SHA-256
  `41d31af2702b83a020fe373a8bb1c18234ccda10ba5829abd98b360a387ba43d`.
- GDB snapshot: `/tmp/wateros-la-round6-gdb.log`, SHA-256
  `495309643b98310c0060619008e62a0537e360d3f9db5d572c336f74d2bb8c4f`.
- QEMU PID 758223 was explicitly terminated after QMP freeze and capture.

## LoongArch passive diagnostic attempt 7 — passed

- Updated passive diagnostic kernel SHA-256:
  `ef2f0516a1a26a573d0d2b9d19645d3508831c19b947bba08b75f355b50fc2e2`;
  another fresh image used the audited script.
- The build had a temporary low-activity interval after the UEFI/axklib group,
  but two vCPUs remained materially active and compilation resumed. Full result:
  `status=OK`, `rc=0`, `elapsed_s=530.12`, `run=OK`, with normal completion.
- No passive stall report appeared. This pass is diagnostic only and does not
  count toward final acceptance.

## LoongArch passive diagnostic attempt 8 — failed: low-rate futex livelock

- Same passive kernel and fresh-image recipe. Full build stopped after the
  UEFI/axklib group; all vCPUs returned to timer-only host activity.
- Frozen atomics showed CPU0 had received `0xf432` timer interrupts and all
  physical CPUs were idle. However, the syscall total was `0x9e31a`, the last
  syscall was 98 (`futex`), and the old unchanged-total detector had just reset
  its window. After resuming for exactly ten seconds and freezing again, the
  total rose to `0x9e414`: 250 futex calls, or about 25 calls/second, without any
  serial/build progress.
- This refines the failure from a permanent missed wake to a low-rate futex
  wake/retry/reblock livelock. Passive diagnosis is updated to inspect a
  30-second rate window; fewer than 2,000 syscalls while every online CPU is
  instantaneously idle triggers task, CPU, and futex registry snapshots.
- Serial log: `/tmp/wateros-la-passive-round8.log`, SHA-256
  `1040f0b62718bfbd129e4def31be68dd3e3ad6e631df03b749b5ab64fb7bc385`.
- GDB snapshot: `/tmp/wateros-la-round8-gdb.log`, SHA-256
  `8efbbe216606ac8fd982fc5db1a83540b43f567ee2281232a6a3f324cbf09609`.
- QEMU PID 760752 was explicitly terminated after measurement.

## LoongArch passive diagnostic attempt 9 — passed

- Low-rate passive diagnostic kernel SHA-256:
  `ca219924e59a4d2b7af4222a55929098ccea0962eecd0358487252a04b4d71d0`;
  another fresh image used the audited script.
- Full result: `status=OK`, `rc=0`, `elapsed_s=523.47`, `run=OK`, with normal
  nested and outer completion. No low-rate stall snapshot was emitted.
- This pass remains diagnostic and does not count toward final acceptance.

## LoongArch passive diagnostic attempt 10 — passed

- Same low-rate passive kernel and fresh-image recipe. Full result:
  `status=OK`, `rc=0`, `elapsed_s=523.19`, `run=OK`, with normal completion.
- The low-rate snapshot still did not emit. The remaining instantaneous
  all-CPUs-idle condition can miss a 25-Hz futex loop whenever the CPU0 sample
  coincides with its brief running phase. It is removed for the next diagnostic
  run; the 30-second low syscall-rate threshold remains.

## LoongArch passive diagnostic attempt 11 — failed: snapshots filtered by log level

- Passive kernel SHA-256:
  `88bcb88dfc0643ed2995b882f81d8df1310d976066a78104e6adcb3b253ca826`;
  fresh image and audited script used.
- The low-rate state reproduced after UEFI. GDB showed the 30-second sampler
  repeatedly completed its windows and updated the baseline; observed deltas
  were about 600 syscalls/window, below the 2,000 threshold. No lines appeared
  because final-online logging displays `ERROR`, while all pre-existing stall
  diagnostics use `WARN`/`INFO`.
- QEMU PID 764556 was explicitly terminated after confirming the logging
  mismatch. The next diagnostic build temporarily promotes only stall snapshot
  messages to `ERROR`; normal runtime behavior is unchanged.

## LoongArch passive diagnostic attempt 12 — failed: stable timed-futex hang captured

- Passive kernel SHA-256:
  `e61bd7f8a1f280d49b43b739211093be80bf0ebb892f85cd5e84676e1cd5d339`;
  a freshly decompressed image used the audited source script plus the single
  EOF `cat /work/buildstorm.run.out`.
- The build stopped after the UEFI/axklib group. Repeated 30-second snapshots
  showed all 12 physical CPUs running their idle tasks, every run queue empty,
  and exactly 300 additional syscalls per window. All of those late calls were
  syscall 98 (`futex`); task 308's schedule count rose by 60 per window while
  its CPU-tick count remained fixed.
- Task 308 repeatedly timed out on private futex address `0x0652dd80` in address
  space `0x960cfb50`. The last futex wake in that address space targeted a
  different address, `0x417e7fb0`, requested an unlimited wake, and found no
  waiter. Tasks 329 and 330 remained blocked on two other futex queues; several
  children of task 308 were already exited but unreaped. TLB pending/completed
  counters, IPIs, scheduler current-task records, and run queues were all
  consistent, excluding the earlier TLB/shootdown and scheduler-cache theories.
- Full serial log: `/tmp/wateros-la-passive-round12.log`, SHA-256
  `24ef8eeb081c06622b1e59b4136b9a978173940fc857c3807eb677102f9aed55`
  (796 lines, 130,554 bytes). Earlier GDB sampling log:
  `/tmp/wateros-la-round12-gdb.log`, SHA-256
  `ed898fa590ac640c7ec8b6d88f0daf6fffc45e0c834fc73a439ab8f832c0a71f`.
- QEMU PID 766900 was stopped explicitly through QMP after the stable state was
  documented. The next diagnostic adds the exact futex key beside every queue
  and waiter, plus process pid/tid/role/state/`clear_child_tid` for every task.

## LoongArch diagnostic attempt 13 — failed: launch used ordinary kernel by mistake

- A fresh image and the audited source-plus-EOF-`cat` script were used. The
  newly built kernel SHA-256 was
  `d0cf082cf13f2a3893cbbc28f4ff318ad5b3c40f3eefb618628b94956933def5`.
- The build again stopped after `Compiling uefi v0.37.0` / `axklib v0.7.5`.
  A two-second host-thread delta showed every vCPU at only 1--4% timer-level
  activity and the serial file remained fixed at 17,115 bytes.
- The launch was then audited and found to have been built with plain
  `make kernel-la-final`, omitting `EXTRA_FEATURES=stall-debug`. Consequently
  this ordinary kernel contained no passive sampler and could not emit the new
  task/futex fields. QEMU PID 768940 was explicitly stopped through QMP; this
  configuration-error run is retained but does not count as acceptance or as a
  completed diagnostic experiment.
- Serial log: `/tmp/wateros-la-passive-round13.log`, SHA-256
  `380693f0592b69a81e61ce534010d0731d18c9db95545d7ad5b5d9593ce48abc`
  (294 lines, 17,115 bytes). The next build explicitly passes
  `EXTRA_FEATURES=stall-debug` and verifies the diagnostic marker exists in the
  resulting ELF before launch.

## LoongArch passive diagnostic attempt 14 — failed: leaked pipe writer identified

- The kernel was built with `make kernel-la-final EXTRA_FEATURES=stall-debug`;
  SHA-256 `3f9e93111f035b279e6fd36521ee6439aa145e1d4963e39d8ecafc96e1527217`.
  The diagnostic marker was verified in the ELF before boot. A freshly
  decompressed image used the audited source script plus exactly one EOF
  `cat /work/buildstorm.run.out`.
- The build stopped after the UEFI/axklib group. Repeated snapshots again
  showed all 12 physical CPUs idle, empty run queues, and exactly 300 futex
  syscalls per 30-second window. Cargo task 308 was only performing its
  100-ms timed wait; futex wake counters no longer advanced.
- The blocked process chain exposed the causal resource leak. Task 343
  (`build-script-bu`, PID 166) remained in `pipe-read`, waiting for EOF from its
  child. That direct child, task 379 (`rustc`, PID 182), was already
  `Exited(0)`; its second thread, task 395 (`ctrl-c`), was also `Exited(0)`.
  A process whose complete thread group is exited was therefore still holding
  the pipe writer that its parent awaited.
- Source tracing matched this state: `exit_group_with_wait_code` remotely
  marked blocked siblings exited. A blocked syscall can own stack-local
  pipe/socket leases, so remote task removal skips their Rust destructors.
  Running siblings later took the task-only trap exit path, which also skipped
  syscall-owned FD and per-thread cleanup. The resulting live writer prevents
  pipe EOF even though the child process is a zombie.
- Full serial log: `/tmp/wateros-la-passive-round14.log`, SHA-256
  `0c50da5d958fe9d3c4b752b7266eb3df72a15a7debe5aa9e5d5fa42afffe7fce`
  (701 lines, 238,325 bytes). QEMU was stopped explicitly through QMP after
  preserving the stable snapshots.

## LoongArch root-fix diagnostic attempt 15 — passed

- Applied the lifecycle fix before this run: exit-group now interrupts sibling
  waits and lets each sibling unwind its own syscall stack, while the
  trap-return `ProcessState::Exiting` path invokes syscall-owned per-thread
  cleanup instead of the task-only exit routine.
- Diagnostic kernel SHA-256:
  `9bbfbf860a4d6448318edeac29a70373bc9f1fe865d3c91a59e7e42c77904958`.
  The run used another freshly decompressed LA image and the audited script
  source plus exactly one EOF `cat /work/buildstorm.run.out`.
- Result: `TOOLCHAIN_RESULT status=OK`, `MINIBUILD_RESULT status=OK`, and
  `BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=12 elapsed_s=519.03
  artifact=target/loongarch64-unknown-linux-musl/release/arceos-helloworld
  bytes=1714568 run=OK`. The appended output contained `Hello, world!`, the
  buildstorm command exited 0, and WaterOS reported all commands finished.
- Passive snapshots fired during legitimate low-activity compilation windows,
  but at least one CPU remained non-idle and compilation continued through the
  earlier UEFI/axklib freeze point. The old stable combination of an exited
  rustc thread group with its parent permanently in `pipe-read` did not recur.
- Full serial log: `/tmp/wateros-la-rootfix-diagnostic-round15.log`, SHA-256
  `8460992342f9c630466a7709714518602670de067b84b192c00b565c34715b6b`
  (768 lines, 125,128 bytes). This is a diagnostic confirmation and does not
  count toward the final ordinary-kernel consecutive acceptance total.

## Final LoongArch acceptance round 1/3 — passed

- Removed all temporary diagnostics and built the ordinary final kernel with
  `make kernel-la-final`; SHA-256
  `bd1d8763eaa63000e64017544b4342289a6f7cb6745e741a4e2349693488a31e`.
  The ELF was checked to contain no `stall-debug` marker.
- Deleted the previous raw image, freshly decompressed the audited LA gzip, and
  installed the audited source-plus-single-EOF-`cat` script (inode mode 0755,
  size 7,578 bytes).
- Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=12 elapsed_s=526.98 ... bytes=1714568 run=OK`; appended nested
  output contained `Hello, world!`; WaterOS reported all commands finished.
- Full serial log: `/tmp/wateros-la-final-acceptance-1.log`, SHA-256
  `8b3504a183c617c13daab5221bf67a53ec49e55a76dbb817a662616ebe2cdb63`
  (588 lines, 32,772 bytes). Consecutive ordinary-kernel LA total: 1/3.

## Final LoongArch acceptance round 2/3 — passed

- Deleted round 1's raw image, freshly decompressed the audited LA gzip, and
  reinstalled the same audited 0755, 7,578-byte source-plus-single-EOF-`cat`
  script. Used the identical ordinary kernel from round 1.
- Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=12 elapsed_s=526.04 ... bytes=1714568 run=OK`; appended nested
  output contained `Hello, world!`; WaterOS reported all commands finished.
- Full serial log: `/tmp/wateros-la-final-acceptance-2.log`, SHA-256
  `ac7fbd53405df27b0456cfbc0ea2d23379c22518e92589112994da540e01f5ab`
  (589 lines, 32,823 bytes). Consecutive ordinary-kernel LA total: 2/3.

## Final LoongArch acceptance round 3/3 — passed

- Deleted round 2's raw image, freshly decompressed the audited LA gzip, and
  reinstalled the same audited 0755, 7,578-byte source-plus-single-EOF-`cat`
  script. Used the identical ordinary kernel from rounds 1 and 2.
- Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=12 elapsed_s=520.52 ... bytes=1714568 run=OK`; appended nested
  output contained `Hello, world!`; WaterOS reported all commands finished.
- Full serial log: `/tmp/wateros-la-final-acceptance-3.log`, SHA-256
  `655a312f8892cde5e73d1c5ca3169ceb9313e54e68671905dd5f3a089ffd106b`
  (588 lines, 32,772 bytes). Consecutive ordinary-kernel LA total: 3/3;
  LoongArch final acceptance is complete.

## RISC-V acceptance input audit

- Clean RV gzip SHA-256:
  `cba87f43ae569bcf2b8e4614f75cec1bf51bedb2804626fe466fcce3861df6f1`.
- RISC-V rounds use the same authoritative recovered source and the same
  source-plus-single-EOF-`cat` temporary script already audited above.

## Final RISC-V acceptance round 1/3 — passed

- Built the ordinary final kernel with `make kernel-rv-final`; SHA-256
  `2c9402bf63c41cb70956c1da1a7d7e5d5f3fbef6dc376308e1af7f3f29ad38df`.
  The ELF was checked to contain no `stall-debug` marker. Deleted the remaining
  LA raw image before freshly decompressing the audited RV gzip and installing
  the audited 0755, 7,578-byte script.
- Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=8 elapsed_s=545.66 ... bytes=1681000 run=OK`; appended nested
  output contained `Hello, world!`; WaterOS reported all commands finished.
  QEMU was launched with 12 vCPUs; the RV kernel exposed its configured 8
  online cores to the guest workload.
- Full serial log: `/tmp/wateros-rv-final-acceptance-1.log`, SHA-256
  `134c0fbb68e5286707259fce0dd898da2eedc0546526432b1ead05dd6e00bc25`
  (566 lines, 36,124 bytes). Consecutive ordinary-kernel RV total: 1/3.

## RISC-V acceptance attempt 2 — failed: guest kernel heap ENOMEM

- Used a newly decompressed audited RV image and the identical ordinary kernel
  and script recipe. Toolchain and minibuild passed, but the full build returned
  `BUILDSTORM_RESULT mode=multi status=FAIL rc=1 cores=8 elapsed_s=424.91
  run=FAIL`; outer WaterOS completed normally.
- The first concrete error was Cargo failing to write `libc` and `std` rmeta
  files with `Cannot allocate memory (os error 12)`. Immediately beforehand,
  WaterOS logged `[heap] high water: used=120905224 free=13312504
  cap=134217728`. This is an explicit kernel-heap capacity/fragmentation failure,
  not the LoongArch pipe-EOF hang and not an exit-path panic. No nested boot was
  attempted, so the appended EOF `cat` correctly reported the absent output.
- Full serial log: `/tmp/wateros-rv-final-acceptance-2.log`, SHA-256
  `bcb7eb63e3dab4cbd23afa909cb214c7a4f5099c55f0a484b21fe6d2c78ae08c`
  (421 lines, 28,468 bytes). Consecutive RV total resets to 0/3.

## RISC-V acceptance attempt 3 — failed: heap ENOMEM reproduced

- A second independent fresh RV image reproduced the same failure earlier in
  the build: `libcore.rlib` archive creation returned `Cannot allocate memory
  (os error 12)` after `[heap] high water: used=120821533 free=13396195
  cap=134217728`. Final result was `status=FAIL rc=1 elapsed_s=234.59
  run=FAIL`; outer WaterOS completed normally.
- Full serial log: `/tmp/wateros-rv-final-acceptance-3.log`, SHA-256
  `96cf53fb87b3c218ffe206341a27415284ce72cb837ee96777eaa1b41871c972`
  (358 lines, 25,727 bytes). Consecutive RV total remains 0/3.
- Two fresh-image reproductions establish that 128MB is below the reliable
  working set for the RV native build. The follow-up changes only the RISC-V
  configured heap to 256MB and expands the TLSF first-level bitmap to represent
  that pool; LoongArch remains at 128MB.

## Final RISC-V post-heap-fix round 1/3 — passed

- Rebuilt both ordinary final kernels. RV kernel SHA-256:
  `3c541a2975227b24abdf55eec42707c730296fd02805f13e895e925e6d7216c3`;
  LA kernel SHA-256:
  `bacc48d32f975e1b12d02e239d6c1be25fd859cffad340d530e073141a0fdfdd`.
  ELF section inspection confirmed RV `.kernel.heap=0x10000000` (256MB) and
  LA `.kernel.heap=0x08000000` (128MB).
- Used another freshly decompressed RV image with the audited script recipe.
  Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=8 elapsed_s=550.47 ... bytes=1681000 run=OK`; nested output
  contained `Hello, world!`; WaterOS reported all commands finished. Neither
  heap high-water warning nor ENOMEM appeared.
- Full serial log: `/tmp/wateros-rv-heapfix-acceptance-1.log`, SHA-256
  `163c0c815d76837240eaf7d512e1e7575ac2dbb47598d31dab1a8db91913b6a5`
  (565 lines, 36,015 bytes). Consecutive post-fix RV total: 1/3.

## Final RISC-V post-heap-fix round 2/3 — passed

- Deleted round 1's raw image, freshly decompressed the audited RV gzip, and
  installed the identical audited script recipe. Used the same ordinary
  256MB-heap RV kernel.
- Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=8 elapsed_s=549.28 ... bytes=1681000 run=OK`; nested output
  contained `Hello, world!`; WaterOS reported all commands finished. No heap
  high-water warning or ENOMEM appeared.
- Full serial log: `/tmp/wateros-rv-heapfix-acceptance-2.log`, SHA-256
  `0fed6c4abc0b65bbee64fc0963c870e806c1e6b160860636b44ab513d4c55d75`
  (565 lines, 36,015 bytes). Consecutive post-fix RV total: 2/3.

## Final RISC-V post-heap-fix round 3/3 — passed

- Deleted round 2's raw image, freshly decompressed the audited RV gzip, and
  installed the identical audited script recipe. Used the same ordinary
  256MB-heap RV kernel.
- Result: toolchain and minibuild OK; `BUILDSTORM_RESULT mode=multi status=OK
  rc=0 cores=8 elapsed_s=561.92 ... bytes=1681000 run=OK`; nested output
  contained `Hello, world!`; WaterOS reported all commands finished. No heap
  high-water warning or ENOMEM appeared.
- Full serial log: `/tmp/wateros-rv-heapfix-acceptance-3.log`, SHA-256
  `0c27d227c47a725deef4012258d94b6ec1eb1a5e6c3cc897c45365e672c982f7`
  (565 lines, 36,014 bytes). Consecutive post-fix RV total: 3/3; RISC-V final
  acceptance is complete.

## Final verification summary

- `make kernel-rv-final`: passed; final RV SHA-256
  `3c541a2975227b24abdf55eec42707c730296fd02805f13e895e925e6d7216c3`.
- `make kernel-la-final`: passed; final LA SHA-256
  `bacc48d32f975e1b12d02e239d6c1be25fd859cffad340d530e073141a0fdfdd`.
- `make rv_check`: passed with pre-existing warnings only.
- `make la_check`: passed with pre-existing warnings only.
- `readelf -SW`: RV `.kernel.heap` is 256MB and LA `.kernel.heap` remains
  128MB.
- `git diff --check`: passed.
- Final runtime acceptance: three consecutive ordinary-kernel fresh-image
  passes on LoongArch and three consecutive post-fix ordinary-kernel
  fresh-image passes on RISC-V. Every counted pass has toolchain/minibuild OK,
  `BUILDSTORM_RESULT status=OK rc=0 run=OK`, nested `Hello, world!`, and outer
  `all commands finished`.

## Cleanup and shutdown

- Confirmed no WaterOS QEMU remained, then deleted both decompressed acceptance
  images. A post-reboot audit found neither `os/sdcard-la-acceptance.img` nor
  `os/sdcard-rv-acceptance.img`; only the two original Downloads `.img.gz`
  inputs remain, with their previously audited hashes unchanged.
- Direct execution of `~/funny-script/syyu_and_shutdown.sh` returned 126 because
  the file mode is 0644. Running the unchanged file through `bash` executed its
  update commands, but its final `shutdown --now` was rejected by this host's
  shutdown implementation. The equivalent `sudo shutdown now` then returned 0.
- The pre-shutdown boot ID was
  `313318c67db94e75a585b6a2ac45ea9e`; the completion audit after restart reports
  `ebaf9c60-1273-4481-bf23-3ca3e72d73af`, proving that the requested shutdown
  sequence took effect. `/tmp` was cleared by that restart, so the persistent
  per-run line/byte counts, result markers, and SHA-256 hashes above are the
  retained test record.
