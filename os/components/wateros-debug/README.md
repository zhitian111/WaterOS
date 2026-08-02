# wateros-debug

`wateros-debug` 是 WaterOS 与主机 GDB 工具之间的低层诊断 ABI。它不依赖调度器、
内存管理或日志系统，避免调试代码反向制造锁依赖。

启用根 crate 的 `gdb-debug` feature 后，crate 会导出：

- `WATEROS_DEBUG_STATE`：header 内含架构和 build ID 的版本化每 CPU 状态与事件环；
- `WATEROS_DEBUG_BUILD_ID`：用于验证本地 ELF 与远端内核是否匹配；
- `WATEROS_DEBUG_FRAME_POINTERS`：证明该 ELF 由强制 frame pointer 的调试构建产生；
- `record_event`、`publish_cpu_state`：仅执行固定大小的原子写入，不分配、不打印。

每个 CPU 有两个状态槽。写入方先填充非活动槽，再以 Release 顺序发布槽号；GDB
即使把 CPU 停在更新中间，也只会读取上一份完整状态。事件环的 `sequence` 最后
发布，sequence 不匹配的记录应被主机忽略；环回卷会累计 `dropped_events`，提醒
报告中的时间线只覆盖最近 256 项。

CPU 状态包含当前 task kind/state、调度策略、nice、等待目标、地址空间、五类
runqueue、timer/switch/syscall/trap/IPI 计数和最近 trap/syscall。关键锁以“类别 +
对象地址”标识；首批接入 scheduler、process registry、futex registry、frame
allocator、地址空间/TLB、VFS fd registry、网络栈和 klog。`TrackedMutex` 用 RAII
保证真实锁 owner 与诊断区同步。

关闭 `enabled` feature 时，公共记录函数会被编译为空操作，普通内核不携带热路径
诊断开销。

根 crate 的 `gdb-fault-injection` 另提供测试专用故障钩子；它不属于本低层 ABI，
且不会进入普通 `gdb-debug` 或 release 构建。
