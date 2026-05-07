# wateros-abi 公共 API 快照

## 用途

描述 **`wateros-abi`** 聚合层在默认 feature 下对内核与用户态共享契约的 **真实再导出**：系统调用号表、参数包、错误码与用户返回值编码。契约细节以 **`wateros-abi-api-v0`** 源码为准。

## 事实来源

- [`os/components/wateros-abi/Cargo.toml`](../../os/components/wateros-abi/Cargo.toml)
- [`os/components/wateros-abi/src/lib.rs`](../../os/components/wateros-abi/src/lib.rs)
- [`os/components/wateros-abi/abi-api/api-v0/`](../../os/components/wateros-abi/abi-api/api-v0/)

## Feature（默认构建）

| Feature | 说明 |
|---------|------|
| **`default`** | `api-v0` + **`impl-linux-riscv64`**。 |
| **`api-v0`** | 传递 **`impl-dummy/api-v0`**（依赖链占位）；根 **`lib.rs`** 中模块均受 **`cfg(feature = "api-v0")`** 保护。 |
| **`impl-linux-riscv64`** | 在 **`syscall_number`** 子模块下提供 **`ActiveSyscallNumberTable`** 类型别名，指向 **`LinuxRiscv64`** 具体号表。 |
| **`impl-dummy`** | 空；不与 **`impl-linux-riscv64`** 同时用于号表别名（默认走 linux 表）。 |

## 聚合层导出（`#[cfg(feature = "api-v0")]` 下）

| 模块 | 说明 |
|------|------|
| **`user_ret`** | `pub use api_v0::user_ret::*`：`UserRet`、`SyscallResult` 及 `from_success` / `from_error` / `from_kernel_result` 等。 |
| **`errno`** | `pub use api_v0::errno::*`：`ErrNo`、`KernelResult`、`raw`/`user_ret` 及常用 **`E*`** 常量。 |
| **`syscall_number`** | `SyscallNumber`、`SyscallNumberTable`；**`#[cfg(feature = "impl-linux-riscv64")]`** 下 **`ActiveSyscallNumberTable`**（默认即 Linux riscv64 表类型）。 |
| **`syscall_args`** | `pub use api_v0::syscall_args::*`：`SyscallArgs`、`SyscallPacket` 及 `from_regs` / `arg` / `as_regs` 等。 |

根 crate **无**独立 `pub fn`；**无**未加 `api-v0` 的顶层导出。

## 缺口说明

- **`impl-dummy`** 实现 crate 已进依赖图，但默认号表与 **`wateros-syscall`** 等消费方以 **`impl-linux-riscv64`** 为主；更换 ABI 表需同时调整 feature 与调用方假设。

## 维护要求

聚合 **`lib.rs`** 再导出、`[features]` 或 **`api-v0`** 契约变更时，同步更新本文件、**`docs/exports/features/wateros-abi.md`**（若涉及能力叙述）及依赖 **`wateros-abi`** 默认 feature 的组件文档（如 **`wateros-syscall`**）。
