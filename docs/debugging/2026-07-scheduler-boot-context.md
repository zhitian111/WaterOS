# Scheduler 启动上下文错配调试记录

## 现象

在单核启动 LTP musl bring-up 时，系统可执行一段用户测试后发生内核态指令页故障：

```text
pc=0x0 ra=0x0 returns_to_user=false
```

CPU Dashboard 显示 current task 是 busybox bring-up runner，但随后 trap 侧看到
的 scheduler current task 是 CPU0 的 idle task。早期还观察到一个不可能的日志顺序：

```text
[stage-busybox] BEGIN
entered runner
...
kernel runner enqueued
[stage-busybox] END
```

runner 不可能在 `spawn_kernel_task()` 返回前正常开始运行，因此这是定位的关键证据。

## 定位过程

1. 首先为上下文切换增加临时诊断，检查 next context 的空指针、零返回地址和
   heap 地址；并在 fatal trap 中记录 `ra/sp`。
2. `pc=0, ra=0` 表明内核返回链已经损坏，而不是用户地址空间或 LTP 本身的错误。
3. 临时比较实时 `sp` 与 scheduler cache 所指 TCB 的内核栈范围，得到“实际仍在
   boot/runner 栈上，但 cache 已指向 idle”的证据。
4. 结合日志顺序发现：boot code 仍运行在 early-kernel stack 时，创建 runner 会设置
   本地 `need_resched`；`dispatch_reschedules()` 随即调用 `schedule_reschedule()`。
   scheduler 此时仅逻辑上把 current 预置为 idle，却把真实 boot stack 保存进了 idle
   context。后续恢复该上下文时，返回地址被破坏为 0。

## 修复

`CPUState` 增加 `boot_context_active`：

- scheduler 初始化后为 `true`；此时允许任务入队，但不允许本地 reschedule 真正切换；
- `schedule_reschedule()` 在该阶段直接保留请求；
- `prepare_first_switch()` 在从 boot stack 发起首个真实 `__switch` 前将其置为 `false`；
- 首次选择会从已积累的 ready queue 中直接选出 runner。

同时修正普通 `schedule()` 的顺序：先将当前任务转换为 Ready/Blocked/Exited（Exit
可唤醒 waiters），再调用 `pick_next_runnable()`。这样 next 的选择基于完整的 ready
集合，而不会在退出路径先选 idle、再把 waiter 入队。

## 验证点

修复后，启动日志必须满足：

```text
kernel runner enqueued
[stage-busybox] END
entered runner
```

即 runner 只能在 boot stack 已通过 `run_first_task()` 切出之后执行。

## 清理

定位期间加入的实时 `sp`/TCB 栈范围断言和额外日志已移除，避免把 RISC-V 特定诊断
长期放入通用 scheduler 热路径。保留启动上下文状态机与调度顺序修复。
