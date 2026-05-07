# wateros-utils 功能快照

## 用途

记录 **`wateros-utils`** 当前作为占位/示例工具 crate 的状态，避免被误认为已提供通用跨组件工具库。

## 事实来源

- `os/components/wateros-utils/Cargo.toml`
- `os/components/wateros-utils/src/lib.rs`
- `os/components/wateros-utils/src/asm/`（若存在且未接入构建）

## 当前已具备能力

- 聚合 **`src/lib.rs`** 仅导出示例函数 **`add(u64, u64) -> u64`** 与单元测试。
- 仓库内可能存在 **RISC-V 汇编**示例文件（如 **`print_rigister.S`**），**未**经 **`global_asm!`**、**`build.rs`** 或 **`lib.rs`** 接入当前 crate 构建产物。

## 架构模式

- **无** **`[features]`**、**无** **`api-v0`**、**无** **`impl-*`** 分层。

## 明确未覆盖

- 与内核 bring-up、驱动或调试输出集成的真实工具 API。
- 汇编例程的 FFI 与安全封装。

## 维护要求

若引入真实工具 API 或接入汇编，同步更新本文件与根 **`os/Cargo.toml`** 依赖说明（当前根 crate 仍依赖 **`wateros-utils`** 作为轻量占位）。
