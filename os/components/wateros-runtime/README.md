# wateros-runtime

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-runtime` 是内核最底层的运行时服务聚合 crate。它提供输出、日志、panic 终止与全局
堆，但不拥有调度、设备策略或平台寄存器操作。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按统一模块名再导出 panic / console / logging / heap_allocator / serial；不含独立逻辑。 |
| 控制台 | `runtime-console/` | `Console` 后端抽象与统一输出 API（`write_fmt` / `write_str` / `write_raw_bytes`、`print!` / `println!`、`AnsiColor`）。 |
| 日志 | `runtime-logging/` | `log` crate 的全局 logger 与级别选择（`init`、着色输出）。 |
| panic | `runtime-panic/` | `panic_handler`：输出、flush、平台 shutdown。 |
| 堆分配器 | `runtime-heap-allocator/` | 全局堆（TLSF / 链表后端）、统计与 OOM。 |
| 串口 | `runtime-serial/` | 字符设备 UART 的运行期再导出（QEMU `virt` UART0）。 |

## 实现说明

- 本 crate 只做**再导出**，不引入新类型或初始化顺序；根 crate 负责按引导顺序调用各子模块的
  `init` / `panic_handler` 等入口。
- 初始化顺序：arch/platform early init → early console 可写 → `logging::init()` →
  `heap_allocator::init()` → 可能分配内存的 driver / VFS / task / 用户态 bring-up。
- `panic_handler` 可在最早阶段挂接，但若 console 或 reset 尚不可用，只能尽力输出并挂起；不能
  在 AP 上重复初始化 heap 或重复注册 logger。
- 输出面：`runtime-console` 的 `write_fmt` / `write_str` / `write_raw_bytes` 是唯一推荐的输出
  接口；它们不选择硬件，`impl-platform-console` 时整条输出进入 `platform::console`（持有跨
  CPU UART 锁）。调用输出前不得持有 scheduler、VFS、allocator 或 driver 层锁。
- logging：`impl-trace` … `impl-error` 中选择日志最大级别，多个同时启用时取最详细一档；
  `log::set_logger` 是全局一次性注册。
- heap：默认 `rlsf::Tlsf`（O(1) alloc/dealloc），`impl-linked-list-allocator` 可切回
  `LockedHeap`；堆大小与对齐来自 `base-config` 的 MM 配置；`interrupt_guard` 禁止本 CPU 中断
  重入；AP 使用堆前必须等待 BSP 单线程初始化完成。
- `impl-platform-console` 连接 QEMU/platform 控制台；`serial-uart-virt` 导出 QEMU virt UART
  字符设备 API，与 early console 是不同层。

## 调用链路

初始化顺序：

```text
arch/platform early init
  → early console 可写
  → runtime::logging::init()
  → runtime::heap_allocator::init()
  → driver / VFS / task / 用户态 bring-up
```

输出与分配：

```text
println! / log 宏
  -> runtime::console::write_fmt / logging 着色输出
  -> platform::console（跨 CPU UART 锁）

alloc / dealloc
  -> runtime::heap_allocator（TLSF / 链表后端 + interrupt_guard）
```

## 各实现功能

### runtime-console / 控制台

- `console-api/api-v0`：`Console` 后端约束。
- `console-impl/impl-platform-console`：连接真实 platform console（跨 CPU UART 锁）。
- 根 crate：`write_fmt` / `write_str` / `write_raw_bytes`、`print!` / `println!` 宏、`AnsiColor`
  （ANSI SGR 转义）。

### runtime-logging / 日志

- `lib.rs`：`init()` 注册全局 logger 并设置 `log::max_level`；`impl-trace`…`impl-error` 取最
  详细一档。
- `logger.rs`：内部着色 logger，把记录转发到 runtime-console。

### runtime-panic / panic

- `panic_handler`：panic 输出、flush、平台 shutdown；不返回；console/reset 均按 best-effort
  处理。

### runtime-heap-allocator / 全局堆

- `lib.rs`：`init`、`heap_mem_stats()`（`HeapMemStats`：used/free/capacity）、OOM。
- `backend_tlsf.rs`：默认 TLSF 后端（O(1)）。
- `backend_linked_list.rs`：`impl-linked-list-allocator` 后端。
- `interrupt_guard.rs`：分配期间禁止本 CPU 中断重入。
- `stress.rs`：`heap_fragmentation_stress_report()` 碎片压力报告。

### runtime-serial / 串口

- 再导出字符设备注册表与 QEMU `virt` UART：`Ns16550Port`、`RegisterLayout`、
  `init_default_virt_uart`、`register_uart_character_device`、`QEMU_VIRT_UART0_BASE` 等。
- 这是已注册字符设备的再导出，不是 early console；内核日志应继续经
  `runtime-console → platform::console`。

## Feature

- `impl-platform-console`：连接真实 platform console（正常内核配置使用）。
- `impl-trace` 至 `impl-error`：选择日志最大级别；多个同时启用时取最详细一档。
- `heap-tlsf` / `heap-linked-list`：堆后端选择（互斥）；`heap-stress`：初始化时压力测试。
- `serial-uart-virt`：导出 QEMU virt UART 字符设备 API。
