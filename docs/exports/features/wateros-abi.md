# wateros-abi — 已实现功能

事实来源：`os/components/wateros-abi/Cargo.toml`、`os/Cargo.toml`（`abi/impl-linux-generic64`）。

## 用途

定义用户态与内核共享的 syscall ABI：错误码、参数包布局、调用号抽象与返回值编码。

## Feature 与能力

| Feature | 状态 | 说明 |
|---------|------|------|
| `default` | 空 | 聚合 `lib.rs` 不导出任何公共模块 |
| `api-v0` | 已实现 | 启用 `wateros-abi-api-v0` 的类型重导出 |
| `impl-dummy` | 占位 | 工作区依赖占位，仅含 `add` 烟测 |
| `impl-linux-generic64` | 已实现 | Linux asm-generic 64 位调用号表；主线 `qemu-riscv64-opensbi` / `qemu-loongarch64-virt` 均启用 |

## 子 crate 能力

### wateros-abi-api-v0

- `ErrNo`：Linux errno 常量子集与 `KernelResult<T>` 别名
- `UserRet` / `SyscallResult`：成功非负、失败 `-errno` 编码
- `SyscallNumber` / `SyscallNumberTable`：调用号 newtype 与符号名 trait（早期 busybox 子集）
- `SyscallArgs` / `SyscallPacket`：`repr(C)` 参数包，槽位数来自 `MAX_SYSCALL_ARGS`

### wateros-abi-impl-linux-generic64

- `LinuxGeneric64`：`SyscallNumberTable` 具体实现，覆盖文件 I/O、进程、调度、内存、信号、socket 等常用号
- `SELECT` 使用 `usize::MAX` 哨兵（asm-generic 无独立 `select` nr）
- 编译期与单测校验号表唯一性

### wateros-abi-impl-dummy

- 仅占位，不参与运行时 ABI

## 缺口

- `default` feature 下对外无 API，调用方须显式启用 `api-v0` 及具体 impl
- `SyscallNumberTable` 未覆盖全部 Linux 调用；表中有号不代表内核已实现
- RISC-V 64 与 LoongArch64 共用一张表，架构分叉后需拆专用 impl
- 无独立 per-arch 调用号 impl（如 riscv64 专用表）
