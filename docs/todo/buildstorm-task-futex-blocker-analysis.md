# BuildStorm 全线程阻塞问题分析报告

## 1. 交付范围

本文交付给成员 B，聚焦 task、scheduler、wait queue、futex 等待协议及线程退出清理。
当前现象可以确定为“用户任务最终全部阻塞，最后活跃系统调用为 futex”，但尚不能直接
断言唯一根因就是 futex 实现。修复时必须先补诊断，再逐项关闭竞态。

基准提交：

- `837d6b79`：按地址空间隔离 private futex key。
- `dde1ff5a`：补充 `exit_group` 的兄弟线程资源释放。
- `d11a6f26`：隔离继承 fd 的操作锁，已消除此前 fd 并发导致的内核崩溃。

不要恢复“给无限期 futex wait 周期性超时”的临时轮询方案。该方案未解决问题，还会
改变 Linux futex 语义并放大调度开销。

## 2. 复现方法

在 `os/` 下构建：

```bash
make kernel-rv-final
```

用干净 overlay 启动，避免修改基线镜像：

```bash
qemu-img create -f qcow2 -F raw \
  -b "$PWD/sdcard-rv-pub.img" /tmp/wateros-buildstorm.qcow2

qemu-system-riscv64 -machine virt -kernel ./kernel-rv-final \
  -m 8G -nographic -smp 8 -bios default \
  -drive file=/tmp/wateros-buildstorm.qcow2,if=none,format=qcow2,id=x0 \
  -device virtio-blk-device,drive=x0,bus=virtio-mmio-bus.0 \
  -device virtio-net-device,netdev=net -netdev user,id=net \
  -rtc base=utc -no-reboot
```

镜像中的决赛脚本为 `/glibc/buildstorm_testcode.sh`。当前能够看到：

```text
rustc 1.98.0-nightly ...
cargo 1.98.0-nightly ...
BUILDSTORM_TOOLCHAIN ok
```

但无法到达 `BUILDSTORM_MINIBUILD`。诊断构建停止后，dashboard 长时间保持：

```text
SYSCALL total=24461 last=98 (0x62)
```

RISC-V syscall 98 是 `futex`。此时没有 USER 任务处于运行或就绪状态，各 CPU 基本均为
IDLE，内存和 fd 数量稳定。因此这不是单纯的 QEMU TCG 执行较慢，而是用户线程没有被
重新唤醒。

## 3. 已排除项

- 工具链路径、符号链接和 `/proc/self/exe` 已通过 `rustc/cargo --version` 验证。
- private futex 使用相同虚拟地址发生跨进程冲突的问题已由 `837d6b79` 修复。
- 旧 fd 共享操作锁及裸指针别名问题已由 `d11a6f26` 修复；本次停止未再出现旧的
  allocator/page-cache 崩溃。
- final 构建默认关闭 dashboard，因此不是 dashboard 串口输出导致业务线程停顿。
- 测试使用全新 qcow2 overlay，不能用基线镜像已损坏解释该现象。

## 4. P0：futex 存在 SMP 丢失唤醒窗口

涉及文件：

- `os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs`
- `os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/key.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/lib.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs`

`FutexHub::wait_while()` 会先检查用户 futex 值，再调用
`wait_current_while()`。后者关闭的只是当前 CPU 中断，然后先执行条件检查，之后才在
全局 scheduler 锁内调用 `schedule_wait()` 入队。远端 CPU 不受本地关中断影响，因此
存在以下合法交错：

1. CPU A 最后一次检查 futex 值，条件仍要求等待。
2. CPU B 修改 futex 值并调用 wake；此时等待队列为空，wake 返回 0。
3. CPU A 随后才加入等待队列并切走。
4. 若不再发生新的 wake，CPU A 永久睡眠。

这是源码可确认的竞态窗口。普通 mutex、pthread join 和 condition variable 都可能
触发它。

### 修复要求

- 为 wait queue 增加“准备等待、条件复查、提交阻塞/取消等待”的原子协议。
- waiter 在发布其等待意图后必须再次检查 futex 值；waker 必须与该发布动作建立同一
  同步关系。
- 不能在持有 scheduler 全局锁时执行可能失败或触发复杂用户页访问的
  `copy_from_user`。
- 推荐给每个 futex key/队列增加短时自旋锁或 generation 序号，并由 scheduler 提供
  `prepare_wait`、`commit_wait`、`cancel_wait` 一类接口。锁只覆盖入队状态转换，
  不能跨上下文切换持有。
- timed wait、requeue、wake-one、wake-all 必须使用同一协议，不能只修无限等待路径。
- 明确定义锁顺序，例如 `futex table/key lock -> scheduler lock`，并检查反向获取。

## 5. P0：信号递送失败可能破坏线程组退出路径

一次周期性重查实验曾暴露：

```text
[trap] killing user task (signal frame setup failed)
cause=Exception(UserEnvCall) pc=0x1c6c430c fault_addr=0x0
task_id=65 parent_id=Some(64) state=Running
```

涉及文件：

- `os/src/trap_handler.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs`

`deliver_pending_signal()` 可能因用户栈地址、signal frame 写入、altstack 状态或 signal
registry 状态返回错误。目前 trap 层只记录统一的 “signal frame setup failed”，随后
杀死当前任务，丢失了 errno、信号号、目标栈和线程组信息。若被杀的是持锁线程或 worker，
其他线程可能继续在 futex/join 上等待。

### 修复要求

- 在失败点记录 errno、signal、task/tid/pid、handler、restorer、原 SP、frame SP、
  altstack 范围和地址空间标识。
- 检查 signal 杀死非 leader 线程后，线程组状态、父进程通知、`clear_child_tid` 和
  robust futex 清理是否完整。
- 增加“阻塞于 futex 时收到信号”“`SA_RESTART`”“`SA_ONSTACK`”回归测试。

## 6. P1：线程退出和 futex 清理需要联合审计

涉及文件：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/robust.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/futex.rs`

重点检查：

- `CLONE_CHILD_CLEARTID` 在线程正常退出、信号退出和 `exit_group` 三条路径中是否都先
  写 0，再以正确地址空间 scope 唤醒等待者。
- robust list owner-died 写入及 wake 是否使用退出线程原地址空间，而非执行清理 CPU
  上“当前任务”的地址空间。
- 非 leader 线程退出后，其 task registry、wait queue、fd runtime binding 和 signal
  state 是否按一次且仅一次释放。
- `wake_user_addr()` 同时尝试 private/shared key 是否可能遗漏真实 key，或产生重复状态
  转换。

## 7. P1：等待队列生命周期与远端重调度

`FutexHub::wake()` 先从表中复制 `WaitQueue`，释放 futex table 锁后才执行
`wake_one()`，随后又尝试 `cleanup_empty_queue()`。需要检查：

- `try_release_empty()` 与并发 waiter/waker 是否可能提前回收并复用 `WaitQueueId`。
- 被 dequeue 的任务是否必然执行 `finish_wait()`、进入 ready queue，并向其目标 CPU
  发送 reschedule IPI。
- wake 返回 0 时究竟是队列为空、任务状态不存在，还是队列 ID 已失效。
- task 被 signal/exit 移除时，timeout queue 和显式 wait queue 是否都同步删除。

相关代码位于：

- `os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs`
- `os/components/wateros-ipc/ipc-waitqueue/waitqueue-impl/impl-task/src/lib.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/wait.rs`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs`

## 8. 建议诊断信息

先增加低频计数器和可按 task/key 过滤的事件，不要在每次 futex 调用上直接打印：

- wait：task/tid/pid、futex key、期望值、queue ID、generation、入队前后状态。
- wake：key、请求数、实际唤醒数、被唤醒 task、目标 CPU、是否发送 reschedule IPI。
- 退出：退出原因、`clear_child_tid` 地址、写入结果、private/shared 各自唤醒数、robust
  清理结果。
- scheduler：所有用户任务阻塞时，输出 task state、wait target、queue ID 和最后 CPU。
- signal：记录第 5 节列出的 signal frame 失败上下文。

诊断应能回答：停止时每个用户线程正在等哪个 key、对应 key 最后一次 wake 发生在何时、
wake 是否早于 waiter 入队。

## 9. 推荐拆分与提交顺序

每项独立验证并提交，避免一次修改 futex、signal 和退出路径：

1. `[debug] add task wait and futex transition diagnostics`
2. `[test] add SMP futex lost-wake regression`
3. `[fix] make futex wait publication atomic with wake`
4. `[fix] preserve wait queue lifetime during concurrent wake`
5. `[debug] report signal frame delivery failures precisely`
6. `[fix] complete non-leader signal exit cleanup`
7. `[test] cover clear-child-tid and robust futex thread exit`

## 10. 验收标准

- 双核以上反复运行 futex ping-pong、mutex、condvar 和 broadcast 压力测试，不允许依赖
  周期性超时自愈。
- 多个地址空间在相同虚拟地址使用 private futex 时互不影响。
- 非 leader 线程正常退出、信号退出、持 robust mutex 退出后，pthread join 均返回。
- 信号能中断 futex wait，`SA_RESTART` 和 altstack 行为正确。
- `make kernel-rv-final` 通过。
- 8 核 QEMU 中 BuildStorm 超过原停止点 `24461`，出现
  `BUILDSTORM_MINIBUILD ok`，并继续到 `BUILDSTORM_COMPILE`。
- final 构建不依赖 dashboard 或高频调试日志，且无用户任务永久留在 wait queue。

