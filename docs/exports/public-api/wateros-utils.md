# wateros-utils — 公共 API

事实来源：`wateros-utils/src/lib.rs`。

## 当前导出

```rust
pub fn add(left: u64, right: u64) -> u64
```

占位函数，带 `#[inline]`，仅供 crate 内单测。

## 未导出

- `src/asm/riscv/print_rigister.S`：`print_register` 符号，未通过 `global_asm!` 或 build 脚本编入 crate

## 依赖关系

- `#![no_std]`
- 无 `[dependencies]`
- 根 `wateros` 默认依赖，但主线代码尚未消费其 API

## 后续预期

聚合层注释说明：此处将收纳与平台弱耦合的纯工具；当前阶段无稳定对外契约。
