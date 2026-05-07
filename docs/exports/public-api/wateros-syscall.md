# wateros-syscall 公共 API 快照

## 用途

描述一级组件 **`wateros-syscall`** 在根 crate **`wateros`** 中的真实导出：trap/用户态约定可见的 **C ABI 分发入口**，以及其对 **`wateros-abi`**、**`wateros-task`**、**`wateros-runtime-console`** 的依赖关系。本 crate **无**聚合子模块门面，仅暴露上述符号供链接与平台 trap 路径调用。

## 事实来源

- [`os/components/wateros-syscall/Cargo.toml`](../../os/components/wateros-syscall/Cargo.toml)
- [`os/components/wateros-syscall/src/lib.rs`](../../os/components/wateros-syscall/src/lib.rs)
- [`os/src/main.rs`](../../os/src/main.rs)（`extern crate syscall as _` 等链接方式）

## Feature 与依赖

| 项 | 说明 |
|----|------|
| **`default`** | 空数组 `[]`；无根级 feature 开关。 |
| **`abi`** | 固定 `features = ["impl-linux-riscv64"]`，与默认 **`ActiveSyscallNumberTable`**（Linux riscv64 号表）对齐。 |
| **`task` / `console`** | 分别依赖 **`wateros-task`**、**`wateros-runtime-console`**（默认 OpenSBI 控制台路径），**不**经过 **`wateros-runtime`** 聚合 crate。 |

## 聚合层（根 crate）导出

| 项 | 说明 |
|----|------|
| **`__wateros_syscall_dispatch_current`** | `pub extern "C" fn(syscall_nr, arg0..arg5) -> isize`，`#[unsafe(no_mangle)]`；由平台 trap 在用户态系统调用路径上调用。 |
| **（内部私有）** | `dispatch_write`、`dispatch_brk`、`dispatch_current_syscall` 等不对外 `pub`。 |

## 已实现的系统调用语义

在 **`ActiveSyscallNumberTable`** 解析的调用号下，当前分支行为为：

| 调用 | 行为摘要 |
|------|----------|
| **`YIELD`** | `task::yield_now()`，成功返回 `0`。 |
| **`EXIT` / `EXIT_GROUP`** | `task::exit_current(exit_code)`（`exit_code` 取自 `arg0`）。 |
| **`WRITE`** | 仅 **`fd == 1` 或 `fd == 2`**：将用户缓冲经 **`console::write_raw_bytes`** 输出；长度上限约 4MiB；否则 **`EBADF`**。缓冲区为 **`unsafe`** 切片构造，**依赖**上层对用户指针合法性的约定。 |
| **`BRK`** | 单调递增 **`USER_BRK_FAKE`** 原子桩：`brk(0)` 查询、`brk(addr)` 仅接受不小于当前顶；**非**真实 VMA，后续应对接 MM。 |
| **其它** | 一律 **`ENOSYS`**。 |

## 缺口与后续替换点

- 覆盖面极小；大量 ABI 表中已有号未实现。
- **`BRK`** 与 **`WRITE`** 的安全与完整用户态地址校验叙事依赖 trap/MM 协作，文档与实现中已标注为桩或受限路径。
- 若需与「WaterOS 自有 ABI」号表并存，应通过 **`wateros-abi`** 的 feature 组合调整，并同步本 crate 的 **`abi`** 依赖 feature。

## 维护要求

分发符号、已支持 syscall 集合或 **`abi`** 依赖 feature 变化时，同步更新本文件、**`docs/exports/features/wateros-syscall.md`**（若存在）与 **`docs/architecture/snapshot.md`** 中 trap/链接相关叙述。
