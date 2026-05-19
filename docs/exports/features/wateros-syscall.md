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
- 子 crate 依赖 **`wateros-abi`**（由 **`impl-riscv64` / `impl-loongarch64`** 打开 **`impl-linux-generic64`** 号表）、**`wateros-task`**、**`wateros-ipc`**、**`wateros-base`**、**`wateros-runtime-console`**，用于在 trap 入口处分发到任务、pipe fd 与控制台。

## 当前已具备能力

- **分发入口**：导出 **`__wateros_syscall_dispatch_current`**（或等价符号，以 `lib.rs` 为准），供架构 trap 处理在用户态系统调用异常时调用。
- **已实现号码与行为**（`dispatch_current_syscall` 语义摘要）：
  - **YIELD**：`task::yield_now()`。
  - **EXIT / EXIT_GROUP**：`task::exit_current(exit_code)`。
  - **READ**：支持 pipe read endpoint；stdin 暂未接真实输入。
  - **WRITE**：fd 1、2 走 **`console::write_raw_bytes`**；pipe write endpoint 走 **`wateros-ipc`** pipe；其余 fd 或方向错误返回 **`EBADF`**；`len == 0` 成功返回 0；过长参数返回 **`EINVAL`**。
  - **CLOSE**：关闭动态 fd；pipe endpoint 关闭会通知底层 pipe。
  - **PIPE2**：创建一对 pipe fd，支持 `O_NONBLOCK`，未知 flags 返回 **`EINVAL`**。
  - **BRK**：RISC-V + `user_aspace_ptr` 时走 Sv39 用户地址空间；否则回落 **`brk(0)` 查询式桩**与单调递增假顶。
  - **MMAP / MUNMAP / MPROTECT**：RISC-V + `user_aspace_ptr` 路径接入 `mm::user_sv39_syscall`。
  - **WAITPID**：基于 task 最小 `parent_id` 与 child-exit wait queue，阻塞等待子任务退出并回收 zombie。
  - **其余号码**：统一 **`ENOSYS`**。

## 明确未覆盖

- 完整 Linux 兼容 syscall 面（仅子集，且 brk/mmap 等多数仍在 ABI 契约层或桩状态）。
- 与 **`wateros-vfs` / `wateros-fs`** 的文件描述符打通。
- fd registry 暂在 syscall crate 内部，尚未处理 fork/dup 继承、任务退出时自动关闭、或 fd limit。
- **`wateros-ipc`** 的 signal、futex 等上层语义。

## 维护要求

分发表、trap 接线或依赖组件（task、console、abi）行为变化时，同步更新本文件与 **`docs/architecture/snapshot.md`**。
