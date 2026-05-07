# wateros-runtime 功能快照

## 用途

记录 **`wateros-runtime`** 聚合层对 **控制台**、**日志**、**panic**、**全局堆分配器** 的再导出，以及各子 crate 的默认 feature 行为（以各子 **`Cargo.toml`** 为准）。

## 事实来源

- `os/components/wateros-runtime/Cargo.toml`（聚合包无 **`[features]`**）
- `os/components/wateros-runtime/src/lib.rs`
- `runtime-console/`、`runtime-logging/`、`runtime-panic/`、`runtime-heap-allocator/`
- `os/feature-tree.txt`（若与实机 **`Cargo.toml`** 不一致，以子 crate 为准并考虑同步修正树）

## 聚合导出

- **`panic_handler`**：来自 **`runtime-panic`**（打印到控制台后调用固件 **`shutdown`** 循环）。
- **`console::*`**：**`wateros-runtime-console`**（**`print!` / `println!`**、**`ConsoleHandle`**、**`show_logo`** 等）。
- **`logging::*`**：**`wateros-runtime-logging`**（初始化 logger 并重导出 **`log`** 各等级宏）。
- **`heap_allocator::*`**：**`wateros-runtime-heap-allocator`**（**`#[global_allocator]`**、**`init()`** 使用 **`KERNEL_HEAP_SIZE`** 等）。

## 子 crate 默认行为（摘要）

- **`runtime-console`**：**`default`** = **`api-v0`** + **`impl-firmware-opensbi`**；未启用 OpenSBI 控制台路径时 **`write_raw_bytes`** 可为空操作（**`cfg`** 控制）。
- **`runtime-logging`**：按单一 level feature 注册 **`WaterOSLogger`**；默认以实际 **`Cargo.toml`** 为准（**`feature-tree.txt`** 中若描述为 **`impl-debug`** 与 **`impl-trace`** 双开，可能与当前 **`default`** 不一致，属文档/树漂移需单独对齐）。
- **`runtime-panic`**：无 feature；依赖平台固件 **`reset`**。
- **`runtime-heap-allocator`**：**`default`** = **`impl-buddy-allocator`**；伙伴分配器 + 分配错误 **`panic!`**。

## 明确未覆盖

- 聚合包自身无 feature 切换；跨目标控制台后端扩展需新增子 impl。
- **`feature-tree.txt`** 与 **`runtime-logging`** 的 **`default`** 若不一致，应在维护周期内统一。

## 维护要求

子 crate 默认 feature、panic 策略或堆初始化常量变化时，同步更新本文件与 **`docs/architecture/snapshot.md`**。
