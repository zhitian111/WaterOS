# wateros-debug

[项目首页](../../../README.md) · [内核工程](../../README.md) · [调试工具](../../../docs/tools/debugging.md)

`wateros-debug` 是 WaterOS 与主机 GDB 工具之间的低层诊断 ABI。它不依赖调度器、内存管理或
日志系统，避免调试代码反向制造锁依赖。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 诊断 ABI | `src/lib.rs` | 版本化的每 CPU 状态与事件环、build ID、frame pointer 证明、`TrackedMutex` 与记录/发布入口。 |
| 构建脚本 | `build.rs` | 编译期符号导出。 |

## 实现说明

- 启用根 crate 的 `gdb-debug` feature（本 crate 的 `enabled`）后导出：
  - `WATEROS_DEBUG_STATE`：header 内含架构和 build ID 的版本化每 CPU 状态与事件环。
  - `WATEROS_DEBUG_BUILD_ID`：用于验证本地 ELF 与远端内核是否匹配。
  - `WATEROS_DEBUG_FRAME_POINTERS`：证明该 ELF 由强制 frame pointer 的调试构建产生。
  - `record_event`、`publish_cpu_state`：仅执行固定大小的原子写入，不分配、不打印。
- 每个 CPU 有两个状态槽：写入方先填充非活动槽，再以 `Release` 顺序发布槽号；GDB 即使把 CPU
  停在更新中间，也只会读取上一份完整状态。
- 事件环的 `sequence` 最后发布，sequence 不匹配的记录应被主机忽略；环回卷会累计
  `dropped_events`，提醒报告时间线只覆盖最近 256 项。
- CPU 状态包含当前 task kind/state、调度策略、nice、等待目标、地址空间、五类 runqueue、
  timer/switch/syscall/trap/IPI 计数和最近 trap/syscall。
- 关键锁以“类别 + 对象地址”标识；首批接入 scheduler、process registry、futex registry、frame
  allocator、地址空间/TLB、VFS fd registry、网络栈和 klog。`TrackedMutex` 用 RAII 保证真实锁
  owner 与诊断区同步。
- 关闭 `enabled` feature 时，公共记录函数会被编译为空操作，普通内核不携带热路径诊断开销。
- 根 crate 的 `gdb-fault-injection` 另提供测试专用故障钩子；它不属于本低层 ABI，且不会进入
  普通 `gdb-debug` 或 release 构建。

## 调用链路

记录与发布：

```text
内核热路径
  -> record_event(kind, ...)       // 固定大小原子写入事件环
  -> publish_cpu_state(cpu, ...)   // 填充非活动槽后 Release 发布槽号
GDB 主机
  -> 读 WATEROS_DEBUG_STATE（校验 build ID / frame pointer）
  -> 读取每 CPU 状态与事件环，按 sequence 过滤，报告 dropped_events
```

## 各实现功能

### src/lib.rs / 诊断 ABI

- 常量：`DEBUG_MAGIC = 0x5741_5445_5244_4247`（`"WATERDBG"`）、`DEBUG_ABI_VERSION = 1`、
  `EVENT_CAPACITY = 256`、`HELD_LOCK_CAPACITY = 8`、`NO_TASK = u64::MAX`、`DEBUG_ARCH`
  （riscv64=1 / loongarch64=2）、`ENABLED = cfg!(feature = "enabled")`。
- `DebugEventKind`：18 种稳定事件编号（TaskEnqueue/TaskSwitch/TaskBlock/TaskWake/TaskExit、
  SyscallEnter/Exit、TrapEnter/Exit、Timer、IpiSend/Receive、FutexWait/Wake、TlbShootdown、
  LockContended/Acquire/Release），`#[repr(u16)]` 只能追加、不能复用旧编号。
- `DebugLockKind`：8 类关键锁（Scheduler/ProcessRegistry/FutexRegistry/FrameAllocator/
  AddressSpace/Vfs/Network/Klog），主机报告用“类别 + 对象地址”标识锁。
- `record_event` / `publish_cpu_state`：只做固定大小原子写入，不分配、不打印；`enabled` 关闭时
  编译为空操作。
- `TrackedMutex`：为关键 `spin::Mutex` 添加诊断而不改变调用处 `lock()` 形状；`current_cpu`
  只在 `enabled` 构建调用，普通 release 常量折叠为一次原始 `Mutex::lock()`；字段析构顺序保证
  先真正解锁、再从诊断区删除 owner。
- 双状态槽：写入方先填充非活动槽，再以 `Release` 发布槽号；事件环的 `sequence` 最后发布，
  sequence 不匹配的记录被主机忽略，回卷累计 `dropped_events`（覆盖最近 256 项）。

- `record_event` / `publish_cpu_state`：记录与发布入口。

### build.rs / 构建脚本

- 编译期导出与诊断相关的符号与常量。

## 失败边界

诊断区是有限快照：事件覆盖、held-lock超过容量、CPU停在发布中间或主机ELF build-id不匹配都可能造成不完整报告，不能据“未记录”证明事件未发生。记录路径不得分配、打印或阻塞；回归应注入环回卷、半发布slot、锁容量溢出和错误ELF，并确认关闭feature时热路径为空操作。
