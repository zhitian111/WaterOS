# wateros-runtime

`wateros-runtime` 是内核最底层的运行时服务聚合 crate。它提供输出、日志、panic
终止与全局堆，但不拥有调度、设备策略或平台寄存器操作。

## 模块

| 模块 | 职责 | 关键约束 |
|---|---|---|
| `runtime-console` | 格式化/原始字节输出与 ANSI 颜色 | 一次输出应使用整段缓冲；跨 CPU 串行化在 platform console |
| `runtime-logging` | `log` crate 的全局 logger 与级别选择 | console 就绪后注册；logger 路径不能分配或递归记录日志 |
| `runtime-panic` | panic 输出、flush、平台 shutdown | 不返回；控制台与 reset 均按 best-effort 处理 |
| `runtime-heap-allocator` | 全局分配器、统计与 OOM | BSP 只初始化一次；AP 使用堆前必须等待完成 |
| `runtime-serial` | 字符设备 UART 的运行期再导出 | 不替代 early console |

## 初始化顺序

```text
arch/platform early init
  → early console 可写
  → runtime::logging::init()
  → runtime::heap_allocator::init()
  → driver / VFS / task / 用户态 bring-up
```

`panic_handler` 可在最早阶段挂接，但若 console 或 reset 尚不可用，只能尽力输出并
挂起。不能在 AP 上重复初始化 heap 或重复注册 logger。

## 输出与并发

`runtime-console` 的 `write_fmt`、`write_str`、`write_raw_bytes` 是唯一推荐的输出面。
它们不会选择硬件；有 platform-console feature 时转交给 `platform::console`，后者持有
跨 CPU UART 锁。调用输出前不得持有 scheduler、VFS、allocator 或 driver 层锁。

## Feature

- `impl-platform-console`：连接真实 platform console，正常内核配置使用它。
- `impl-dummy`：仅用于占位/类型检查，实际输出会失败。
- `impl-trace` 至 `impl-error`：选择日志最大级别；若多个同时启用，logging 选择最详细的等级。
- `serial-uart-virt`：导出 QEMU virt UART 字符设备 API；与 early console 是不同层。

详细的结构与数据不变量分别见各子模块 README。
