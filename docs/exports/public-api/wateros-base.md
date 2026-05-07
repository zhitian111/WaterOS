# wateros-base 公共 API 快照

## 用途

描述 **`wateros-base`** 作为 **无 feature 开关** 的基础 crate，向 mm、platform、task 等提供的 **薄类型与单核同步原语**。与板级常量相关的数值见 **`wateros-base-config`**（本 crate 的 **`config`** 模块仅聚合少量与堆等相关的共享常量）。

## 事实来源

- [`os/components/wateros-base/Cargo.toml`](../../os/components/wateros-base/Cargo.toml)
- [`os/components/wateros-base/src/lib.rs`](../../os/components/wateros-base/src/lib.rs)
- 各子模块 `src/*.rs`

## 聚合层导出

| 模块 | 主要公开项 |
|------|------------|
| **`addr`** | **`BasePhysAddr`**、**`BaseVirtAddr`**、**`BasePPN`**、**`BaseVPN`**（各含 **`pub val: usize`**）；**`Into<*mut T>`** 实现（物理/虚拟地址）。 |
| **`boot`** | 类型别名 **`DTBPA`**。 |
| **`config`** | 常量 **`KERNEL_HEAP_SIZE_BIT_WIDTH`**（与 **`wateros-base-config`** 语义应对齐）。 |
| **`cpu`** | 类型别名 **`CPUHartID`**。 |
| **`sync`** | **`pub mod uniprocessor`**；**`UniprocessorSafeCell`**（**`unsafe fn new`**、**`exclusive_access`**）。 |

根 **无** `pub use` 扁平化整个子树；调用方按模块路径引用。

## 维护要求

新增/移动基础类型或改变 **`config`** 与 **`wateros-base-config`** 关系时，同步更新本文件及引用方（如 mm、platform）文档。
