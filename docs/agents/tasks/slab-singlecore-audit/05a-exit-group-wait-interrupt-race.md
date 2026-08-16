# Task 05a：闭合 `exit_group` 与 sibling 入睡竞态

## 任务目标

修复 BuildStorm 在 `[axbuild] ... done` 后偶发不再输出 `BUILDSTORM_RESULT` 的功能错误。
线程组 leader 发布 `ProcessState::Exiting` 时，sibling 可能仍是 Running，并在退出 CPU
发出重调度通知后、IPI 生效前进入 wait/sleep；现有一次性 `interrupt_task` 此时没有等待
队列项可摘除，后续也没有第二次唤醒，遗留的 pipe writer 会让 shell 的 `tee` 永久等不到
EOF。

## 实施方案

1. 在 TCB 中增加仅供线程组退出使用、持续到 TCB 回收的 sticky wait interrupt；它与普通
   signal 的一次性 `interrupt_task` 分离，避免正常运行线程收到信号后错误中断未来无关的等待。
2. 新增 `interrupt_task_for_exit`：在 scheduler 锁内先登记 sticky interrupt；若任务已经
   Blocking/Sleeping，则同时从等待容器摘除并以 `Interrupted` 唤醒；若仍 Running，则请求
   目标 CPU 重调度。
3. `schedule_wait`、`ScheduleReason::Block` 和非零 `ScheduleReason::Sleep` 在同一 scheduler
   锁内、真正入队前检查 sticky interrupt，拒绝入睡并返回 `Interrupted`；标记不会在第一次
   检查时清除，避免 syscall 内部重试再次入睡。
4. `exit_group_with_wait_code` 对每个 sibling 使用新入口；仍由 sibling 自己展开 syscall 栈和
   执行 Rust 析构，不恢复远端强杀或跨 CPU 提前释放 FD/futex/pipe 资源。
5. 同步 task 与 scheduler README 中的等待/退出并发契约。

## 验收方式

功能门禁：

```bash
cd os
cargo test --manifest-path components/wateros-task/task-impl/impl-core/Cargo.toml --features self_test
make rv_check
make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

运行门禁：使用 RISC-V Final pub 镜像副本和 QEMU 9.2.1，至少完成原生
`exit_group01`（若镜像入口可单独选择）以及一轮完整 BuildStorm。完整轮必须同时出现：

```text
BUILDSTORM_RESULT ... status=OK ... run=OK
#### OS COMP TEST GROUP END buildstorm-glibc ####
[busybox-bringup] all commands finished
```

不得出现 `[axbuild] ... done` 后五分钟无 marker、panic、OOM、double free 或残留 QEMU。

本任务附带的竞态探针同时让 8 个 sibling 竞争进入 pipe read、长时间 nanosleep 和
`sched_yield`，leader 直接调用 `SYS_exit_group`。交叉编译后写入镜像的 `/glibc`，再用
`probes/exit-group-wait-interrupt-race.sh` 临时替换 BuildStorm 入口：

```bash
riscv64-linux-musl-gcc -O2 -static -pthread -Wall -Wextra -Werror \
  -o /tmp/wateros-exit-group-race \
  docs/agents/tasks/slab-singlecore-audit/probes/exit-group-wait-interrupt-race.c
```

专项轮必须输出 `EXIT_GROUP_RACE_RESULT status=OK iterations=500`；运行后丢弃该 raw 镜像，
正式 BuildStorm 必须从原 gzip 重新解压另一份 raw，不能复用专项轮镜像。

## 涉及文件与 CodeGraph

- `os/components/wateros-task/task-impl/impl-core/src/tcb.rs`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/registry.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/wait.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/lib.rs`
- `os/components/wateros-task/src/schedule.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs`
- `os/components/wateros-task/readme.md`
- `os/components/wateros-task/task-scheduler/readme.md`
- `docs/agents/tasks/slab-singlecore-audit/probes/exit-group-wait-interrupt-race.c`
- `docs/agents/tasks/slab-singlecore-audit/probes/exit-group-wait-interrupt-race.sh`

```bash
codegraph explore "exit_group_with_wait_code interrupt_task schedule_wait wait_result"
codegraph explore "ScheduleReason::Block ScheduleReason::Sleep finish_wait_after_switch"
codegraph callers "interrupt_task"
```

## 完成后

新增 `history/05a-brief.md`，记录 lost-interrupt 时序、sticky interrupt 不变量、功能矩阵、
RISC-V 运行 marker 和尚未覆盖的压力场景。本任务单独提交，不能与 slab Task 05 混合。
