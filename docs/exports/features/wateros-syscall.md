# wateros-syscall 功能快照

## 用途

记录 **`wateros-syscall`** 一级组件在默认构建下的系统调用分发与用户态契约对接范围，便于与 **`wateros-abi`**、**`wateros-task`**、**`wateros-vfs`**、平台 trap 路径对照。

## 事实来源

- `os/components/wateros-syscall/Cargo.toml`
- `os/components/wateros-syscall/src/lib.rs`
- `os/components/wateros-syscall/src/dispatch.rs`
- `os/components/wateros-syscall/src/sys/`（各 `sys_*` 实现）
- `os/Cargo.toml`（根依赖 `syscall`）
- `os/src/main.rs`（`extern crate syscall as _` 链接侧引用）
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs`（对分发符号的调用关系，以仓库当前代码为准）

## 聚合层与依赖

- 根 crate **`wateros`** 依赖 **`wateros-syscall`**（包名 `syscall`），平台 feature 启用 **`impl-riscv64`** / **`impl-loongarch64`**。
- 子 crate 依赖 **`wateros-abi`**、**`wateros-task`**、**`wateros-ipc`**、**`wateros-base`**、**`wateros-mm`**；**`fd-session`** 下另依赖 **`wateros-vfs`**，由 **`vfs::fd`** 提供 per-task fd 表。
- **`read` / `write` / `close` / `pipe2`** 经 **`vfs_util::vfs_error_to_errno`** 将 **`VfsError`** 映射为 **`ErrNo`**。

## 当前已具备能力

- **分发入口**：导出 **`dispatch_syscall_from_trap`** 与 **`__wateros_syscall_dispatch_current`**；后者供架构 trap / C ABI 调用，内部将 `syscall_nr` 路由到 **`sys::*`** 实现。
- **已实现号码与行为**（`dispatch_syscall_from_trap` → `sys_*` 语义摘要）：
  - **YIELD**：`task::yield_now()`。
  - **EXIT / EXIT_GROUP**：`task::exit_current(exit_code)`。
  - **READ**：经 **`vfs::fd::with_current_io`** 读 pipe 等句柄；stdin 仍返回 **`EBADF`**（与迁移前一致）。
  - **WRITE**：fd 1/2 与 pipe 写端经 VFS 句柄；非法 fd 返回 **`EBADF`**。
  - **CLOSE**：经 **`vfs::fd::close_fd`** 关闭动态 fd 并调用句柄 `close`。
  - **PIPE2**：创建 pipe 句柄并 **`alloc_fd_for_task`** 登记；支持 `O_NONBLOCK`。
  - **BRK**：RISC-V + `user_aspace_ptr` 时走 Sv39 用户地址空间；否则回落桩。
  - **MMAP / MUNMAP / MPROTECT**：RISC-V 主线且有效 `user_aspace_ptr` 时拼合 `MmapOps`。
  - **WAITPID**：基于 task 最小 `parent_id` 与 child-exit wait queue；回收子任务时丢弃其 cwd 槽位。
  - **OPENAT**：VFS `open` + `alloc_fd`；`open` 路径经 **`resolve_open_path`**（per-task cwd）。
  - **GETCWD / CHDIR**：经 **`vfs::cwd`** 读写 per-task 工作目录。
  - **其余号码**：统一 **`ENOSYS`**。

## 明确未覆盖

- 完整 Linux 兼容 syscall 面（仅子集）。
- **`fchdir`**、`openat` 非 **`AT_FDCWD`** 的 `dirfd`、fork 时 cwd/fd 继承策略的完整 Linux 语义。
- fork/dup 继承、任务退出时自动关闭、fd limit。
- **`wateros-ipc`** 的 signal、futex 等上层语义。
- 用户缓冲 **`copy_from_user` / `copy_to_user`** 安全路径（仍依赖 bring-up 约束下的直接切片）。

## 维护要求

分发表、trap 接线或依赖组件（task、vfs、abi）行为变化时，同步更新本文件与 **`docs/architecture/snapshot.md`**。
