# wateros-base 功能快照

## 用途

记录 **`wateros-base`** 作为内核侧最小公共类型与单核同步原语载体的范围，以及独立子包 **`wateros-base-config`** 在常量配置上的角色（syscall 参数上限、MM/QEMU 布局等）。

## 事实来源

- `os/components/wateros-base/Cargo.toml`（根包无 **`[features]`**，当前 **`[dependencies]`** 可为空）
- `os/components/wateros-base/src/lib.rs`
- `os/components/wateros-base/base-config/`

## 聚合导出

- 模块：**`addr`**、**`boot`**、**`config`**、**`cpu`**、**`sync`** 等（以 **`src/lib.rs`** 为准）。
- 根包**不**再导出 **`wateros-base-config`** 为 Rust 模块名；其它 crate 通过 path 依赖 **`base-config`** 读取常量。

## base-config 子包

- 提供 **`syscall`**（如 **`MAX_SYSCALL_ARGS`**）、**`mm`**（堆大小位宽、QEMU virt RAM/MMIO 常量等）等 **`no_std`** 配置面，被 **`wateros-abi`**、**`wateros-mm`** 等引用。

## 架构模式

- **无**典型 **`api-v0` / `impl-*`** 拆分；能力为直接实现的模块代码。

## 明确未覆盖 / 技术债

- **`boot`** 等模块可能仍为最小占位（如仅 DTB 物理基址常量）。
- **`config.rs`** 与 **`base-config/mm.rs`** 间若存在同名常量，应避免语义分叉并在演进中收敛。

## 维护要求

地址类型、同步原语或 **`base-config`** 常量变化时，同步更新本文件及依赖方的功能快照（**`wateros-abi`**、**`wateros-mm`** 等）。
