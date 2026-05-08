# wateros-abi 公共 API 快照

## 用途

描述 **`wateros-abi`** 聚合层在默认 feature 下对内核与用户态共享契约的 **真实再导出**：系统调用号表、参数包、错误码与用户返回值编码；契约细节以 **`wateros-abi-api-v0`** 源码为准。根组件具备 `api-v0`、`impl-dummy` 与 **`impl-linux-generic64`** 实现 crate；**`impl-linux-riscv64`** / **`impl-linux-loongarch64`** 为架构侧 feature 别名，均启用同一张 Linux generic 64-bit 系统调用号表（由 **`LinuxGeneric64`** 提供 **`ActiveSyscallNumberTable`**）。

## 事实来源

- [`os/components/wateros-abi/Cargo.toml`](../../os/components/wateros-abi/Cargo.toml)
- [`os/components/wateros-abi/src/lib.rs`](../../os/components/wateros-abi/src/lib.rs)
- [`os/components/wateros-abi/abi-api/api-v0/`](../../os/components/wateros-abi/abi-api/api-v0/)

## Feature（默认构建）

| Feature | 说明 |
|---------|------|
| **`default`** | `api-v0` + **`impl-linux-riscv64`**（启用 **`impl-linux-generic64`**）。 |
| **`api-v0`** | 传递 **`impl-dummy/api-v0`**（依赖链占位）；根 **`lib.rs`** 中模块均受 **`cfg(feature = "api-v0")`** 保护。 |
| **`impl-linux-generic64`** | 可选依赖 **`wateros-abi-impl-linux-generic64`**；提供 **`LinuxGeneric64`** 号表实现。 |
| **`impl-linux-riscv64`** / **`impl-linux-loongarch64`** | 均等价于启用 **`impl-linux-generic64`**，供 RISC-V / LoongArch 路径在聚合层选择同一号表。 |
| **`impl-dummy`** | 占位；与 **`impl-linux-generic64`** 解耦，由 feature 链按需组合。 |

## 聚合层导出（`#[cfg(feature = "api-v0")]` 下）

| 模块 | 说明 |
|------|------|
| **`user_ret`** | `pub use api_v0::user_ret::*`：`UserRet`、`SyscallResult` 及 `from_success` / `from_error` / `from_kernel_result` 等。 |
| **`errno`** | `pub use api_v0::errno::*`：`ErrNo`、`KernelResult`、`raw`/`user_ret` 及常用 **`E*`** 常量。 |
| **`syscall_number`** | `SyscallNumber`、`SyscallNumberTable`；**`#[cfg(feature = "impl-linux-generic64")]`** 下 **`ActiveSyscallNumberTable`** → **`LinuxGeneric64`**。 |
| **`syscall_args`** | `pub use api_v0::syscall_args::*`：`SyscallArgs`、`SyscallPacket` 及 `from_regs` / `arg` / `as_regs` 等。 |

根 crate **无**独立 `pub fn`；**无**未加 `api-v0` 的顶层导出。

## 缺口说明

- **`impl-dummy`** 实现 crate 已进依赖图，但默认号表与 **`wateros-syscall`** 等消费方以 **`impl-linux-riscv64`** 为主；更换 ABI 表需同时调整 feature 与调用方假设。

## 维护要求

聚合 **`lib.rs`** 再导出、`[features]` 或 **`api-v0`** 契约变更时，同步更新本文件、**`docs/exports/features/wateros-abi.md`**（若涉及能力叙述）及依赖 **`wateros-abi`** 默认 feature 的组件文档（如 **`wateros-syscall`**）。
