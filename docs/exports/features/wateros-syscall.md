# wateros-syscall 功能快照

## 用途

记录 **`wateros-syscall`** 一级组件在默认构建下的系统调用分发与用户态契约对接范围，便于与 **`wateros-abi`**、**`wateros-task`**、平台 trap 路径对照。

## 事实来源

- `os/components/wateros-syscall/Cargo.toml`
- `os/components/wateros-syscall/src/lib.rs`
- `os/Cargo.toml`（根依赖 `syscall`）
- `os/src/main.rs`（`extern crate syscall as _` 链接侧引用）
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs`（对分发符号的调用关系，以仓库当前代码为准）

## 聚合层与依赖

- 根 crate **`wateros`** 依赖 **`wateros-syscall`**（包名 `syscall`），默认启用子 crate 的 **`default`** feature。
- 子 crate 依赖 **`wateros-abi`**（`impl-linux-riscv64` 号表）、**`wateros-task`**、**`wateros-runtime-console`**，用于在 trap 入口处分发到任务与控制台。

## 当前已具备能力

- **分发入口**：导出 **`__wateros_syscall_dispatch_current`**（或等价符号，以 `lib.rs` 为准），供架构 trap 处理在用户态系统调用异常时调用。
- **已实现号码与行为**（`dispatch_current_syscall` 语义摘要）：
  - **YIELD**：`task::yield_now()`。
  - **EXIT / EXIT_GROUP**：`task::exit_current(exit_code)`。
  - **WRITE**：仅 **fd 1、2** 走 **`console::write_raw_bytes`**；其余 fd 返回 **`EBADF`**；`len == 0` 成功返回 0；过长参数返回 **`EINVAL`**。
  - **BRK**：用户态 **`brk(0)` 查询式桩**（原子保存当前 break；向下调整返回 **`EINVAL`**）；非查询路径为简化更新语义。
  - **其余号码**：统一 **`ENOSYS`**。

## 明确未覆盖

- 完整 Linux 兼容 syscall 面（仅子集，且 brk/mmap 等多数仍在 ABI 契约层或桩状态）。
- 与 **`wateros-vfs` / `wateros-fs`** 的文件描述符打通。
- 与 **`wateros-ipc`** 的 pipe、signal、futex 等上层语义。

## 维护要求

分发表、trap 接线或依赖组件（task、console、abi）行为变化时，同步更新本文件与 **`docs/architecture/snapshot.md`**。
