# wateros-runtime — 已实现功能快照

## 用途

记录 `wateros-runtime` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-runtime/**` 源码与 `Cargo.toml`。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-runtime`（聚合） | 以 `runtime::panic` / `console` / `logging` / `heap_allocator` / `serial` 再导出子能力 | 已实现 |
| `wateros-runtime-panic` | `#[panic_handler]`：着色打印后平台关机 | 已实现 |
| `wateros-runtime-console` + `console-api/api-v0` | `Console` trait、`print!`/`println!`、ANSI 着色 | 已实现 |
| `console-impl/impl-dummy` | 占位：写入即 `unimplemented!` | 已实现（测试/占位） |
| `console-impl/impl-platform-console` | 经 `platform::console` 输出 | 已实现（QEMU 主线） |
| `wateros-runtime-logging` | `log` crate 桥接，按 feature 设级别并着色输出 | 已实现 |
| `wateros-runtime-heap-allocator` | 全局 `GlobalAlloc`：链表或 TLSF 后端 | 已实现 |
| `wateros-runtime-serial` | QEMU virt UART 再导出（`serial-uart-virt`） | 已实现 |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `impl-platform-console` | 控制台 + panic + logging 走平台输出（QEMU 默认） |
| `impl-dummy` | 控制台占位；误输出会 panic |
| `impl-trace` … `impl-error` | 设置 `log` 最大级别（多开时取最安静档） |
| `serial-uart-virt` | 启用 `runtime::serial` 子模块 |

## 已实现能力

- **Panic 路径**：文件/行号 + 消息，红色横幅，调用 `platform::reset::shutdown`。
- **控制台**：泛型 `print`/`prints`、宏 `print!`/`println!`、`write_raw_bytes`（syscall 等原始字节路径）、`show_logo`。
- **开发日志**：`runtime::logging::init` 注册 `WaterOSLogger`；过滤 `ext4_rs` Info 以上噪声。
- **内核堆**：`init`、`heap_mem_stats`、`handle_alloc_error`；中断屏蔽 + 递归分配检测；90% 高水位 warn。
- **堆后端**：默认 `linked_list_allocator`；可选 `impl-tlsf`（互斥 feature）。
- **压测**：`heap_fragmentation_stress_report`（开发/诊断用，结束后 `loop {}`）。
- **串口**：再导出 `wateros-driver` UART API（仅 `serial-uart-virt`）。

## 与 klog 的分工

`runtime-logging` **不**写入 `wateros-klog` 环；需要「上屏 + 进环」时由调用方分别调用 `log::*!` 与 `klog_*!`。

## 缺口与后续

- 控制台 **无输入侧** API（`api-v0` 仅写）。
- `impl-dummy` 不适合生产内核，仅编译占位。
- `CONSOLE_*` syslog action 在 klog 侧为 no-op，未与 runtime 控制台联动。
- 堆 `used` 在 TLSF 后端为估算值，非精确记账。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
