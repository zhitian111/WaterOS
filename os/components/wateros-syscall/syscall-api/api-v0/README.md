# wateros-syscall-api-v0

[项目首页](../../../../../README.md) · [内核工程](../../../../README.md) · [wateros-syscall](../../README.md)

syscall v0 的公共契约 crate。它是 `#![no_std]`，只依赖
`wateros-base-config` 取得 `MAX_SYSCALL_ARGS`，因此能由内核 handler、架构 trap
代码和 task 上下文共同使用。

- `ErrNo` 始终存正 errno；`user_ret()` 才产生 `-errno`。
- `UserRet` 是最终写回用户态寄存器的透明 `isize` 包装。
- `SyscallArgs` 不解释参数，只保存 ABI 顺序的槽位；调用号的语义由 syscall API 定义。
- `SyscallPacket` 仅把编号和参数包组合，方便 trap 层交给分发层。
- `number.rs` 同时保存 Linux generic 64 位调用号常量与 `SyscallNumber`，避免编号
  在 ABI 和 syscall 两处重复维护。

这些类型在 crate 根直接重导出，调用方应优先使用
`wateros_syscall_api_v0::{ErrNo, SyscallArgs, UserRet}`。

详见父目录的 [syscall 文档](../../README.md)。
