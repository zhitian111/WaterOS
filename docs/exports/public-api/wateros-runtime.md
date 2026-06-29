# wateros-runtime — 聚合层公共 API

## 用途

列出根 crate `wateros` 通过 `runtime` 依赖最终使用的对外接口（再导出子 crate 符号）。impl 细节见各子 crate rustdoc。

## 模块树（`wateros-runtime/src/lib.rs`）

```text
runtime::
  panic::panic_handler
  console::*          # 来自 wateros-runtime-console
  logging::*          # 来自 wateros-runtime-logging
  heap_allocator::*   # 来自 wateros-runtime-heap-allocator
  serial::*           # [feature serial-uart-virt] 来自 wateros-runtime-serial
```

## `runtime::panic`

| 项 | 签名 / 说明 |
|----|-------------|
| `panic_handler` | `fn(&PanicInfo) -> !` — 根 `#[panic_handler]` 挂接 |

## `runtime::console`

| 项 | 说明 |
|----|------|
| `AnsiColor` | 终端 SGR 颜色枚举 |
| `ConsoleHandle` | 当前 feature 选中的控制台类型别名 |
| `print` / `prints` | 泛型 `Console` 格式化输出 |
| `write_raw_bytes` | 原始字节写出（无 UTF-8 要求） |
| `show_logo` | ASCII 横幅 |
| `print!` / `println!` | 宏，使用 `ConsoleHandle` |

**`console-api/api-v0`**

| 项 | 说明 |
|----|------|
| `Console` | `fmt::Write + Default` 标记 trait |

## `runtime::logging`

| 项 | 说明 |
|----|------|
| `init` | 注册全局 logger 与 `max_level` |
| `trace!` / `debug!` / `info!` / `warn!` / `error!` | 重导出 `log` 宏 |

## `runtime::heap_allocator`

| 项 | 说明 |
|----|------|
| `init` | 初始化 `HEAP_SPACE` 后端 |
| `heap_mem_stats` | `HeapMemStats { used, free, capacity }` |
| `handle_alloc_error` | `#[alloc_error_handler]` 委托：warn + panic |
| `heap_fragmentation_stress_report` | 压测入口（`!` 返回） |
| `HeapMemStats` | 堆用量快照结构体 |

## `runtime::serial`（`serial-uart-virt`）

再导出 `character_api_v0` 与 `driver::uart` 符号，含 `QemuVirtUart16550`、`init_default_virt_uart` 等。

## 初始化契约（根 crate 责任）

1. `runtime::heap_allocator::init()` — 任何堆分配前
2. `runtime::logging::init()` — 使用 `log!` 前
3. `klog::init()` — 与 logging 独立，见 `wateros-klog` 文档

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
