# wateros-syscall 公共 API 快照

## 用途

描述一级组件 **`wateros-syscall`** 在根 crate **`wateros`** 中的真实导出：trap/用户态约定可见的 **C ABI 分发入口**，以及其对 **`wateros-abi`**、**`wateros-task`**、**`wateros-ipc`**、**`wateros-mm`** 与 **`wateros-runtime-console`** 的依赖关系。本 crate **无**聚合子模块门面，仅暴露上述符号供链接与平台 trap 路径调用。

## 事实来源

- [`os/components/wateros-syscall/Cargo.toml`](../../os/components/wateros-syscall/Cargo.toml)
- [`os/components/wateros-syscall/src/lib.rs`](../../os/components/wateros-syscall/src/lib.rs)
- [`os/src/main.rs`](../../os/src/main.rs)（`extern crate syscall as _` 等链接方式）

## Feature 与依赖

| 项 | 说明 |
|----|------|
| **`default`** | 空数组 `[]`；无根级 feature 开关。 |
| **`abi`** | `default-features = true`；由根 feature（如 **`impl-riscv64`** / **`impl-loongarch64`**）启用 **`abi/impl-linux-riscv64`** 或 **`abi/impl-linux-loongarch64`**，二者均打开 **`impl-linux-generic64`** 号表。 |
| **`task` / `console`** | 分别依赖 **`wateros-task`**、**`wateros-runtime-console`**（默认 OpenSBI 控制台路径），**不**经过 **`wateros-runtime`** 聚合 crate。 |
| **`ipc`** | 由平台 feature 启用 **`wateros-ipc`** 的 **`pipe`** 与对应 arch feature，供最小 fd table 存放 pipe endpoint。 |
| **`base`** | 使用 **`UniprocessorSafeCell`** 保护 syscall crate 内部的 per-task fd registry。 |
| **`mm`** | RISC-V 路径可选启用 **`impl-sv39`**，供 `brk` / `mmap` / `munmap` / `mprotect` 走真实用户地址空间路径。 |

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
| **`READ`** | 支持 pipe read endpoint；空 pipe 按 endpoint 阻塞/非阻塞语义返回，stdin 暂未接真实输入。 |
| **`WRITE`** | **`fd == 1/2`** 输出到 console；pipe write endpoint 写入 IPC pipe；其它 fd 或方向错误返回 **`EBADF`**。缓冲区为 **`unsafe`** 切片构造，**依赖**上层对用户指针合法性的约定。 |
| **`CLOSE`** | 关闭动态 fd；pipe endpoint 会触发对应读端/写端关闭并唤醒等待者。 |
| **`PIPE2`** | 创建 pipe read/write fd 对；支持 `O_NONBLOCK`，未知 flags 返回 **`EINVAL`**。用户态 `pipe()` 包装为 `pipe2(flags=0)`。 |
| **`BRK`** | RISC-V + `user_aspace_ptr` 优先走 Sv39 用户地址空间；无地址空间时回落单调递增 **`USER_BRK_FAKE`** 原子桩。 |
| **`MMAP` / `MUNMAP` / `MPROTECT`** | RISC-V + `user_aspace_ptr` 路径接入 `mm::user_sv39_syscall`；其它平台或无用户地址空间返回 **`ENOSYS`**。 |
| **`GET_TIME`、`GETPID` / `GETTID`、`NANOSLEEP`** | 分别返回当前 tick、当前 task id，或按一个调度 tick 近似睡眠。 |
| **`WAITPID`** | 维护最小父子关系；`pid == -1` 等待任意子任务退出，指定 pid 时要求其父任务为当前任务；退出后回收 zombie 并写回 exit code。 |
| **其它** | 一律 **`ENOSYS`**。 |

## 缺口与后续替换点

- 覆盖面仍是 Linux-like 子集；大量 ABI 表中已有号未实现。
- **`READ` / `WRITE` / `PIPE2`** 已覆盖最小 pipe fd 场景，但 fd registry 仍位于 syscall crate 内部，尚未接入 VFS fd、dup/fork 继承或任务退出时批量关闭。
- 用户指针安全与完整地址校验叙事依赖 trap/MM 协作，当前直接构造 slice 或写用户指针的路径仍是 bring-up 约束下的早期实现。
- 若需与「WaterOS 自有 ABI」号表并存，应通过 **`wateros-abi`** 的 feature 组合调整，并同步本 crate 的 **`abi`** 依赖 feature。

## 维护要求

分发符号、已支持 syscall 集合或 **`abi`** 依赖 feature 变化时，同步更新本文件、**`docs/exports/features/wateros-syscall.md`**（若存在）与 **`docs/architecture/snapshot.md`** 中 trap/链接相关叙述。
