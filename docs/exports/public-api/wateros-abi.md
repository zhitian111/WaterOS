# wateros-abi — 公共 API

事实来源：聚合 `src/lib.rs`（`api-v0` + `impl-linux-generic64` 启用时）。

## 启用条件

根内核 feature `qemu-riscv64-opensbi` / `qemu-loongarch64-virt` 会传递 `abi/impl-linux-generic64`，从而同时启用 `api-v0` 与号表 impl。

## 聚合层重导出

| 模块 | 主要类型 |
|------|----------|
| `errno` | `ErrNo`、`KernelResult<T>`、`ErrNo::*` 常量 |
| `user_ret` | `UserRet`、`SyscallResult` |
| `syscall_number` | `SyscallNumber`、`SyscallNumberTable`；启用 impl 时另有 `ActiveSyscallNumberTable`（`LinuxGeneric64` 别名） |
| `syscall_args` | `SyscallArgs`、`SyscallPacket` |

## api-v0 契约摘要

### ErrNo

- `raw()` → 正数 errno
- `user_ret()` → `-errno`
- 大量 Linux errno 关联常量

### UserRet

- `from_success(usize)`、`from_error(ErrNo)`、`from_kernel_result(SyscallResult)`

### SyscallNumber

- `new(usize)`、`raw()`

### SyscallArgs / SyscallPacket

- `from_regs`、`arg`、`as_regs`、`SyscallPacket::new`

### SyscallNumberTable

- 关联常量：按类别分组的符号化调用号（READ、WRITE、MMAP、FUTEX、SOCKET 等）

## impl-linux-generic64

- `LinuxGeneric64`：零大小类型，实现 `SyscallNumberTable` 全部关联常量

## impl-dummy

- 不通过聚合层导出；仅工作区内部占位

## 未导出 / 需注意

- `default` feature 下 `wateros-abi` 聚合 `lib.rs` 为空壳
- trait 关联常量存在不等于内核 syscall handler 已实现
