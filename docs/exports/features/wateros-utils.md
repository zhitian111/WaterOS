# wateros-utils — 已实现功能

事实来源：`os/components/wateros-utils/Cargo.toml`、`src/lib.rs`。

## 用途

预留通用小工具与可复用例程的聚合位置，与平台类型保持弱耦合。

## 当前能力

| 项 | 状态 | 说明 |
|----|------|------|
| `add(u64, u64)` | 占位 | 仅供 crate 内 `cargo test` 烟测 |
| `src/asm/riscv/print_rigister.S` | 未接入 | 早期 UART 二进制打印辅助，未编入 `lib.rs` |

## 主线依赖

根 `wateros` 以 `utils` 别名默认依赖本 crate（`default-features = true`），但当前内核代码几乎未调用其公共 API。

## 缺口

- 无实际工具函数或数据结构对外导出
- 汇编调试例程未纳入构建
- 无 feature 分层，无 impl 子 crate
