# 跨组件数据结构与生命周期

本手册描述“一个用户任务从创建到回收时，哪些组件各自保存了什么”。它适合排查 fork 后状态错误、exec 泄漏、exit 卡死、地址空间销毁失败，以及压力测试中内核堆持续增长。

## 1. 三种身份不能混用

| 标识 | 含义 | 常见用途 |
| --- | --- | --- |
| `TaskId` | 内核调度实体的稳定键 | fd/cwd/cred/signal thread state、futex waiter |
| `ThreadId` / TID | 用户可见线程编号 | clone 返回值、`set_tid_address`、`/proc` |
| `ProcessId` / PID | 线程组/进程编号 | wait、进程组、SIGCHLD、POSIX record lock |

线程与进程不是一对一。任何以 `TaskId` 建表的子系统都必须逐线程清理；以 PID 为所有者的资源通常在最后一个线程退出时处理。不要用当前 TID/PID 代替内部 `TaskId` 作为注册表键。

## 2. 核心对象所有者

| 对象/状态 | 真相源 | 共享粒度 | 主要释放时机 |
| --- | --- | --- | --- |
| TCB、调度状态、内核栈、trap frame | task/scheduler | 每线程 | reap |
| Process、父子关系、线程组、地址空间描述 | task process registry | 每进程 | 最后线程退出后成为 zombie，wait/reap 删除 |
| 用户页表、VMA、物理帧引用 | MM | 每进程；pthread 共享 | 地址空间最后拥有者释放 |
| fd table / open-file description | VFS | 由 `CLONE_FILES` 决定表共享；OFD 可跨表共享 | exit 清表；OFD 最后引用 close |
| cwd/root/umask | VFS cwd registry | 由 `CLONE_FS` 决定 | exit/reap 幂等释放 |
| mount namespace | VFS mount namespace | clone 默认共享，`CLONE_NEWNS` 复制 | 最后 owner 释放 |
| credential | cred registry | fork 复制，pthread 共享 | zombie 保留，reap 最终删除 |
| signal disposition | syscall IPC signal | 进程级；`CLONE_SIGHAND` 约束共享 | exec 重置部分，进程回收释放 |
| pending signal / signal mask | syscall IPC signal | 线程级与进程级并存 | thread exit/reap |
| robust-list、clear-child-tid | syscall IPC/task | 每线程 | exit 时先执行用户可见动作，reap 幂等清表 |
| SysV SHM attachment | syscall IPC + MM | 每任务登记、映射落在地址空间 | exit/exec detach，段按 nattch/删除标志回收 |
| SysV SEM_UNDO | syscall IPC | 每任务 | exit-time 应用 undo |
| futex wait entry | IPC futex | 每等待线程 | 唤醒、超时或 exit cancel |
| epoll/unix socket 辅助索引 | syscall impl | 每任务/fd | fd 继承时同步，exit/reap drop |
| terminal session/foreground pgid | TTY | session/process group | 最后线程退出时 detach |

## 3. 新用户进程：先构造，后发布

bring-up 创建流程在 [`src/user_bringup_common.rs`](../../src/user_bringup_common.rs)，fork/clone 在 [`clone.rs`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/clone.rs)。共同规则是：

```text
分配地址空间/任务对象（scheduler 尚不可运行）
  -> 建立 cred、fd、cwd、mount ns、signal、SHM 等侧表
  -> 所有可能失败的用户指针写回和 pidfd 安装
  -> start_user_task / start_fork_child / start_clone_thread
```

发布前失败必须完整回滚，发布后则不能再假装创建失败。`abort_initialized_fork` 展示了成熟的回滚集合：robust、signal、通用 task runtime resources、credential、task registry；独立 fork 地址空间还必须被释放。

增加新的 task-local 注册表时，必须同时加入：

1. 初始用户任务创建钩子；
2. fork 的复制或共享钩子；
3. pthread clone 的复制或共享钩子；
4. 未发布 child 的 abort 回滚；
5. exit/reap 清理。

## 4. fork 与 pthread clone 的差异

普通 fork：

- MM 通过 `fork_user_aspace` 建立独立地址空间/COW 关系。
- cwd 根据 `CLONE_FS` 复制或共享。
- fd table 与 epoll 根据 `CLONE_FILES` 复制或共享，但 open-file description 引用语义仍保留。
- mount namespace 默认共享，`CLONE_NEWNS` 时复制。
- credential 使用 `fork_cred`。
- SHM attachments 被继承，并在新地址空间建立对应登记。

pthread 典型 `CLONE_VM|CLONE_THREAD|CLONE_SIGHAND`：

- 与线程组共享地址空间，不创建新进程。
- credential 使用 `share_cred`。
- mount namespace共享。
- fd/cwd 是否共享仍由 flags 决定。
- child TID 写回、TLS、clear-child-tid 都是每线程状态。

`CLONE_THREAD` 没有 `CLONE_VM`、或 `CLONE_SIGHAND` 没有 `CLONE_VM` 均应返回 `EINVAL`。新增 flag 时先定义它对上述每一行的共享/复制语义，不能只让参数校验通过。

## 5. clone 创建事务与错误注入点

当前 fork 顺序的关键失败点：

| 失败点 | 必须回滚 |
| --- | --- |
| `fork_user_aspace` | 函数内部不能遗留半成品 |
| 关闭中断失败 | 新地址空间 |
| `fork_current_parented` 失败 | 新地址空间 |
| child snapshot 不存在 | task child、signal、地址空间 |
| parent/child TID 写回 `EFAULT` | 未发布 child 和所有已建资源 |
| signal `on_fork` 失败 | signal state、child、地址空间 |
| pidfd 分配/写回失败 | fd、全部 child 侧表、地址空间 |

审查原则：把 `start_fork_child` 当成 commit。commit 前每个 `return Err` 都要逐项核对当时已经获得的资源；使用 guard 时要确认 Drop 顺序不会在仍持有全局锁时触发复杂析构。

## 6. exec：准备阶段与不可回退点

[`execve.rs`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs) 先装载新 ELF、准备用户栈和 signal state；这些步骤失败时，旧进程映像仍可运行。之后终止兄弟线程并切换资源：

```text
准备 new ELF/aspace/stack（可失败、可回退）
  -> 确保 signal state
  -> terminate_other_threads_for_exec
  -> signal::on_exec
  -> detach 当前 SHM + 清理被终止线程资源
  -> 非 vfork child: drop old aspace
  ===== 不可回退点 =====
  -> close CLOEXEC fd
  -> 应用 setuid/setgid/capability 变化
  -> cred::on_exec
  -> 更新 exe/argv/env/auxv/comm
  -> task::execve_current 安装新 trap frame/aspace
```

越过旧地址空间释放点后，后续错误只能记录并继续完成 exec，不能向用户返回失败；否则当前任务已无可恢复的旧映像。新增 exec hook 时判断它会不会失败：可能失败的准备应移到不可回退点之前，纯提交或幂等操作才放之后。

exec 不创建新 PID/TID，且不会普通地关闭全部 fd；只关闭 `CLOEXEC`。cwd、mount namespace 和大多数 credential 被保留，signal disposition 按 POSIX exec 规则调整。

## 7. exit-time 与 reap-time 必须分开

退出路径在 [`task.rs`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs)，最终资源清理在 [`wait.rs`](../../components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs)。

exit-time 的目标是立即解除会阻塞系统或具有用户可见副作用的资源：

- 写零 `clear_child_tid` 并 futex wake；执行 robust-list 恢复。
- 应用 SysV `SEM_UNDO`，取消 futex wait，detach SHM。
- 释放 cwd、mount namespace owner、fd table、epoll/unix socket 索引。
- 最后线程退出时释放 POSIX record locks、TTY session，通知 SIGCHLD 和 parent waiters。
- 把任务标成 `Exited`，但保留 zombie 所需身份信息。

reap-time 的目标是删除等待者仍可能查询的 zombie 状态：

- 从 scheduler/process registry 取出 `ExitedTask`。
- 删除 robust/signal thread state 的残留。
- 再次调用幂等 runtime cleanup 兜底。
- 最后删除 credential。
- 由地址空间生命周期所有者在最后引用消失时释放页表/VMA/帧。

credential 不能在当前线程的 exit-time 过早删除，因为退出收尾仍可能查询 current credential。相反，fd/pipe 等不能等父进程 reap 才关，否则无人 wait 的 zombie 会永久持有 pipe writer 或文件锁。

## 8. `exit_group` 为什么不能远程直接标记 sibling Exited

兄弟线程可能阻塞在 syscall 中，内核栈上仍持有 pipe/socket lease 或 RAII guard。直接从另一个 CPU 把它改为 `Exited` 会跳过 Rust 析构，造成引用泄漏或永久不见 EOF。

当前实现先发布进程 `Exiting`，然后 interrupt 或请求 sibling reschedule。兄弟线程从阻塞 syscall 解开并回到 trap 边界后，在自己的内核栈上执行标准 exit 清理。这条约束也是排查 “leader 已退出但 stress-ng/forkheavy 卡住” 的首要检查点。

## 9. 地址空间销毁与写回

地址空间销毁不是简单释放页表。对 file-backed shared VMA，必须先处理 dirty 页写回，再解除 PTE/VMA/页缓存/物理帧之间的引用。建议沿以下顺序记录诊断：

```text
谁触发销毁（exec / exit / abort fork / bring-up purge）
  -> aspace_ptr 和进程/线程组是否仍有 owner
  -> VMA 类型、范围、权限、shared/private
  -> resident PTE 与 lazy metadata 是否一致
  -> file page/handle 是否仍有效
  -> writeback 的 offset/length/backend errno
  -> unmap/TLB shootdown
  -> frame 与 VMA 元数据释放
```

禁止用“忽略 writeback 错误”掩盖所有权错误。首先区分：文件已被错误地提前 close、只读/不支持写回、offset 超范围、VMA 重叠/重复销毁、还是后端真实 I/O 失败。销毁路径应尽量完成其余可安全清理并保留第一个错误诊断，否则一次写回失败会演变为地址空间与堆对象泄漏。

## 10. 压力测试泄漏定位

`forkheavy` 中内核 heap 单调上涨时，每轮或固定间隔采样：

- task/process 数：running、blocked、exited、zombie、reaped；
- 地址空间/VMA/PTE/物理帧对象数；
- fd table、OFD、pipe endpoint、epoll watch 数；
- signal/robust/futex waiter/SHM attachment/SEM_UNDO 表项数；
- heap used/free 与最大单次 allocation。

判断方法：

- task 数回落但 heap 不回落：优先查 task 外部侧表或 allocator 碎片。
- zombie 数增长：查 parent wait、SIGCHLD 与 reap 条件。
- task 数回落但 aspace 增长：查 abort/exec/reap 的地址空间 owner。
- fd/OFD 增长：查 clone 回滚、exit-time 清表和共享表 owner refcount。
- futex waiter 增长：查 timeout/signal/exit 三个取消出口。
- used 上升后稳定：可能是缓存/高水位，不等同泄漏；继续跑多轮观察平台期。

## 11. 新增 task-local 状态的模板

在设计说明中先填完此表：

| 问题 | 必填答案 |
| --- | --- |
| registry key | TaskId、TID 还是 PID，为什么 |
| initial spawn | 默认值和失败语义 |
| fork | copy/share/reset |
| pthread clone | copy/share/reset |
| exec | preserve/reset/drop |
| thread exit | 哪些副作用必须立即发生 |
| process last-thread exit | 哪些进程级资源释放 |
| reap | 哪些 zombie 状态最终删除 |
| abort before publish | 如何完整回滚 |
| lock order | 与 task/MM/VFS/IPC 锁的顺序 |
| regression | 哪个测试覆盖成功、失败、并发和重复清理 |

至少编写一个“创建到一半失败”的回归，以及一个“父进程不立即 wait”的回归；只测正常 fork/wait 无法证明生命周期完整。
