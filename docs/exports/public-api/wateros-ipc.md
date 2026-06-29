# wateros-ipc — 公共 API

## 用途

描述根内核与其它一级组件通过 `wateros-ipc` 聚合 crate **实际使用**的导出符号。事实来源：`os/components/wateros-ipc/src/lib.rs`；根 `os/Cargo.toml` 中 `ipc = { package = "wateros-ipc", ... }`，`impl-riscv64`/`impl-loongarch64` 启用 `ipc/all`。

## 顶层模块树

```text
ipc::api              // wateros-ipc-api-v0（占位）
ipc::active_impl      // [feature impl-dummy] 占位 impl
ipc::waitqueue        // 始终可用（默认 feature）
ipc::pipe             // [feature pipe]
ipc::futex            // [feature futex]
ipc::shm              // [feature shm]
ipc::signal           // [feature signal]
```

## `ipc::waitqueue`

| 符号 | 说明 |
|------|------|
| `WaitQueue` | IPC 侧等待队列（`impl-task`） |
| `TaskId`, `TaskTick`, `TaskWaitHandle`, `TaskWaitResult`, `WaitQueueId` | 与 task API 对齐的类型别名 |
| `IpcWaitQueueOps` | 等待队列 trait（api-v0） |

**`WaitQueue` 主要方法：**

| 方法 | 说明 |
|------|------|
| `new()` | 创建队列 |
| `id()` / `wait_handle()` | 编号与句柄 |
| `try_release_empty()` | 空队列时释放 id |
| `wait_current` / `wait_current_for_ticks` | 阻塞等待 |
| `wait_current_while` / `wait_current_while_for_ticks` | 条件等待 |
| `wake_one` / `wake_all` | 唤醒 |
| `requeue_to` | 部分唤醒 + 迁移等待者 |

## `ipc::pipe`（feature `pipe`）

### 类型与常量

| 符号 | 说明 |
|------|------|
| `Pipe` | 内核内部 pipe（`KernelPipe` 实现） |
| `PipeEndpoint` | fd 表端点（`PipeEndpointOps` 实现） |
| `PipeError`, `PipeResult` | 错误与结果 |
| `PipeEndpointKind` | Read / Write |
| `DEFAULT_PIPE_CAPACITY` | 默认缓冲容量 |
| `KernelPipe`, `PipeEndpointOps` | trait 契约（`ipc::pipe::api`） |

### 典型调用

- 自检：`ipc::pipe::test()`
- 创建：`Pipe::new()` / `Pipe::with_capacity(n)`
- fd 对：`PipeEndpoint::pair(nonblocking)`

## `ipc::futex`（feature `futex`）

| 符号 | 说明 |
|------|------|
| `FutexHub` | 全局 futex 表（`FutexHub::global()`） |
| `FutexKey` | 队列键（`from_uaddr` / `from_syscall`） |
| `FutexError`, `FutexResult`, `FutexWaitOutcome` | 错误与等待结果 |
| `KernelFutexOps` | wake / robust 契约 trait |
| `RobustListHead`, `ROBUST_LIST_*`, `FUTEX_*` | robust 布局常量 |

**`FutexHub` inherent 方法（syscall 常用）：**

| 方法 | 说明 |
|------|------|
| `wait_while(key, timeout, condition)` | 条件阻塞 |
| `requeue(from, to, wake_count, requeue_count)` | FUTEX_REQUEUE 语义 |

## `ipc::shm`（feature `shm`）

| 符号 | 说明 |
|------|------|
| `registry()` | `&'static Mutex<ShmRegistry>` |
| `ShmRegistry` | 段与附加管理 |
| `ShmId`, `ShmError`, `ShmResult` | 类型别名与错误 |
| `ShmSegmentInfo`, `ShmAttachInfo` | 元数据快照 |
| `IPC_PRIVATE`, `IPC_CREAT`, `IPC_EXCL`, `SHM_RDONLY` | Linux 常量 |
| `MAX_SHM_SEGMENT_SIZE` | 4 MiB 上限 |

**`ShmRegistry` 主要方法：**

| 方法 | 说明 |
|------|------|
| `create_or_get` | shmget |
| `segment_info` | 查询段 |
| `begin_attach` / `finish_attach` / `cancel_attach_reservation` | shmat 两阶段 |
| `detach` | shmdt |
| `mark_removed` | IPC_RMID |
| `drop_task` / `fork_task` | 生命周期 |

## `ipc::signal`（feature `signal`）

| 符号 | 说明 |
|------|------|
| `with_registry(f)` | 全局注册表访问入口 |
| `SignalRegistry` | 状态机 |
| `SignalSet`, `SignalAction`, `PendingSignal` | 核心类型 |
| `SignalDispatch`, `SignalDelivery`, `SignalError` | 投递与错误 |
| `IntervalTimerSpec` | itimer 规格 |
| `NSIG`, `SIG*`, `SA_*`, `ITIMER_*` | Linux 常量 |

**`SignalRegistry` 主要方法（节选）：**

| 方法 | 说明 |
|------|------|
| `register_process` / `register_thread` | 注册 |
| `fork_process` / `exec_process` / `drop_*` | 生命周期 |
| `get_action` / `set_action` | disposition |
| `update_mask` / `current_mask` / `replace_mask` | 掩码 |
| `begin_sigsuspend` / `end_sigsuspend` | sigsuspend |
| `begin_poll_sigmask` / `end_poll_sigmask` | ppoll |
| `send_thread` / `send_process` | 投递 |
| `take_deliverable` / `has_deliverable` | trap 交付 |
| `set_timer` / `get_timer` / `expire_realtime` / `account_cpu` | 定时器 |

## 未通过聚合层导出的内容

- `ipc-event`（独立占位 crate，未挂 workspace 聚合）
- 各子 crate 内部 `api_v0` / `impl_*` 根模块（除 `active_impl` 条件导出）
- futex/pipe 的 trait 实现细节与 `PipeState` 内部字段

依赖方应使用 `ipc::waitqueue`、`ipc::pipe` 等聚合路径，避免直接依赖 `wateros-ipc-pipe-impl-ringbuf` 等 impl crate。
