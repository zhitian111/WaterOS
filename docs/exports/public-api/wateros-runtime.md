# wateros-runtime 公共 API 快照

## 用途

**`wateros-runtime`** 根 crate **无** 自有 **`[features]`**；通过四个 **`pub mod`** 再导出子 crate：**`runtime-panic`**、**`runtime-console`**、**`runtime-logging`**、**`runtime-heap-allocator`**。各子能力的行为由 **子 crate 的 default feature** 决定（例如控制台默认 **`impl-firmware-opensbi`**，日志默认 **`impl-trace`**，堆默认 **`impl-buddy-allocator`**）。

## 事实来源

- [`os/components/wateros-runtime/Cargo.toml`](../../os/components/wateros-runtime/Cargo.toml)
- [`os/components/wateros-runtime/src/lib.rs`](../../os/components/wateros-runtime/src/lib.rs)
- 各 **`runtime-*`** 子目录 **`src/lib.rs`**

## 聚合层导出

| 模块 | 说明 |
|------|------|
| **`panic`** | **`panic_handler`**（来自 **`wateros-runtime-panic`**）。 |
| **`console`** | **`wateros-runtime-console`** 根 **`pub`**：**`print`**、**`prints`**、**`write_raw_bytes`**、**`show_logo`**、**`AnsiColor`**；**`ConsoleHandle`** 随子 crate 的 **`impl-dummy`** / **`impl-firmware-opensbi`** 切换。**`print!` / `println!`** 宏在子 crate 中 **`#[macro_export]`**，**不**随 **`pub use console::*`** 进入 **`wateros_runtime::console`** 命名空间；调用方常直接 **`use wateros_runtime_console::print!`** 或依赖 prelude 习惯。 |
| **`logging`** | **`init`**；**`trace!`**、**`debug!`**、**`info!`**、**`warn!`**、**`error!`**（来自 **`log`** 再导出）。 |
| **`heap_allocator`** | **`init`**、**`handle_alloc_error`** 等（伙伴堆路径由子 crate feature 控制 **`#[global_allocator]`**）。 |

## 缺口说明

- 根层无法在单一 manifest 上统一开关子能力；需在 **`wateros`** 或依赖侧对 **`wateros-runtime-*`** 传 feature。
- **`write_raw_bytes`** 在未启用固件控制台实现时可能为吞输出占位。

## 维护要求

子 crate 默认 feature 或再导出项变化时，同步更新本文件。
