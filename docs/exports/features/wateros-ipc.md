# wateros-ipc — 已实现功能快照

## 用途

记录 `wateros-ipc` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-ipc/**` 源码与 `Cargo.toml`；根 `wateros` 在 `impl-riscv64` / `impl-loongarch64` 下启用 `ipc/all`。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-ipc`（聚合） | `api`、`waitqueue`、feature 下 `pipe`/`futex`/`shm`/`signal` | 已实现 |
| `wateros-ipc-api-v0` | IPC 顶层 API 占位（`add` 自检） | 骨架 |
| `wateros-ipc-impl-dummy` | 聚合层 `active_impl` 占位 | 骨架 |
| `ipc-waitqueue` + `impl-task` | 对 `wateros_task::WaitQueue` 的 IPC 薄包装 | 已实现 |
| `ipc-pipe` + `impl-ringbuf` | 内核 ring-buffer pipe + fd 端点 | 已实现 |
| `ipc-futex` + `impl-task` | 全局 `FutexHub`、等待队列与 robust 侧表 | 已实现 |
| `ipc-futex` + `impl-dummy` | futex 链接桩（`Nosys`） | 骨架 |
| `ipc-shm` | SysV 段注册表与物理帧生命周期 | 已实现（bring-up 子集） |
| `ipc-signal` | 进程/线程信号状态机 + itimer | 已实现 |
| `ipc-signal` + `impl-dummy` | 信号 impl 占位 | 骨架（未挂聚合） |
| `ipc-event` | 事件 IPC 占位 crate | 骨架（未挂聚合） |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `default` | `api-v0` + `impl-dummy` + `waitqueue` |
| `all` | 再加 `pipe`、`futex`（`impl-task`）、`shm`、`signal` |
| `pipe` | `ipc::pipe::{Pipe, PipeEndpoint, ...}` |
| `futex` | `ipc::futex::FutexHub`（task 实现） |
| `shm` | `ipc::shm::registry()` 等 |
| `signal` | `ipc::signal::with_registry` 与 API 类型 |

## 已实现能力

### 等待队列（`waitqueue`）

- 默认导出 `WaitQueue`，语义委托 `wateros_task`。
- 支持无限/超时等待、条件等待、`wake_one`/`wake_all`、`requeue_to`。
- 空队列 `try_release_empty` 回收 `WaitQueueId`。

### 管道（`pipe` / `impl-ringbuf`）

- 固定容量 ring buffer；默认容量来自 `base_config::ipc::DEFAULT_PIPE_CAPACITY`（4096）。
- 阻塞/非阻塞读写、EOF、BrokenPipe、Interrupted（信号路径）。
- 读/写端引用计数；部分 close 后 dup/继承仍可读写的 shell 管道语义。
- `poll_revents` / `poll_wait_for_ticks` 对接多 fd `poll`。
- `F_SETPIPE_SZ` 空管 resize 子集。

### Futex（`futex` / `impl-task`）

- 按 `FutexKey`（uaddr + private）惰性建队列表。
- `wait_while` 条件等待（闭包由 syscall 提供用户内存复查）。
- `wake` / `wake_all` / `requeue`（迁移等待者到另一键）。
- per-task robust 链表头 `set/get/drop_robust_list`。
- 空队列清理与 `WaitQueueId` 复用。

### 共享内存（`shm`）

- `shmget` 子集：`IPC_PRIVATE` / 命名键、`IPC_CREAT`/`IPC_EXCL`。
- 单段最大 **4 MiB**；页对齐分配与零页初始化。
- `shmat` 两阶段：`begin_attach` / `finish_attach` / `cancel_attach_reservation`（防并发 RMID 竞态）。
- `shmdt`、`IPC_RMID`（延迟删除）、`fork`/`exit` 附加继承与回收。

### 信号（`signal`）

- 进程级：`rt_sigaction` disposition、进程 pending、三类 `setitimer`。
- 线程级：mask、线程 pending、`sigsuspend`/`ppoll` 临时掩码、`sigwait`。
- `kill`/`tkill` 投递分类：ignore、pending、terminate、stop、continue。
- `take_deliverable` 应用 `SA_NODEFER`/`SA_RESETHAND`；`SIGCHLD`/`SIGALRM` 等定时器路径。
- fork 复制 disposition/mask；exec 重置用户 handler；单元测试覆盖 timer 与 pending 合并。

## 与上下层分工

- **syscall 层**：ABI 解码、errno 映射、用户内存访问、futex 条件闭包。
- **vfs**：`PipeEndpoint` 放入 fd 表；dup/fork 继承。
- **mm**：shm 返回 PPN 后由 syscall 映射用户 VA。
- **task/trap**：信号帧交付、`TaskWaitResult::Interrupted`。

## 缺口与后续

- 顶层 `api-v0` / `impl-dummy` 仍为占位，非真实 IPC 契约。
- `ipc-event` 未接入聚合；无 epoll 事件对象。
- futex 共享键（文件/inode）未实现；robust owner-died 写用户字依赖 syscall 串行。
- signal：无实时信号队列深度、altstack、job control 进程组语义。
- shm：无 `shmctl(IPC_STAT)` 完整字段、无 System V msg/sem。
- pipe：无 splice/tee；全局 pipe 数量无硬上限。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
