# wateros-syscall-api-v0

[项目首页](../../../../../README.md) · [内核工程](../../../../README.md) · [wateros-syscall](../../README.md)

syscall v0 是 trap、task 上下文和内核 handler 共同依赖的 `no_std` ABI 数据层。它只依赖 base-config 取得参数槽数，不得引用 VFS、MM、task 或平台实现。

## 文件和类型

| 文件 | 类型/内容 | 约束 |
|---|---|---|
| `number.rs` | `SyscallNumber` 与 asm-generic64 号常量 | 有编号不等于已实现 |
| `args.rs` | `SyscallArgs`、`SyscallPacket` | 只保存原始寄存器位，不解引用 |
| `errno.rs` | `ErrNo`、`KernelResult<T>` | 内核内部始终是正 errno |
| `return_value.rs` | `UserRet` | 唯一的 `-errno` 编码边界 |

crate 根直接 re-export 这些项。普通调用方优先使用 `api_v0::{ErrNo, SyscallArgs, UserRet}`，不要复制一份 errno/调用号表。

## 参数 ABI

`SyscallArgs` 是 `repr(C)` 的 `[usize; MAX_SYSCALL_ARGS]`，槽位顺序与架构 trap 保存的 Linux ABI 参数寄存器一致。RISC-V/LoongArch64 均使用 asm-generic 64 位调用号，但“如何从 trap frame 取 nr/args、把返回写回哪个寄存器”仍属于 arch trap。

`from_regs` 不验证调用号，`arg(idx)` 越界会 panic；handler 只能访问该 syscall 规定的槽位。裸 `usize` 既可能是整数、flag、fd，也可能是用户地址，API 层不解释。所有符号扩展、32 位子字段、pair 参数和结构版本都在 handler 显式完成。

`SyscallPacket { nr: SyscallNumber, args }` 只是组合值，没有验证或所有权语义。保存 packet 用于 syscall restart 时，还必须同时保存原 PC/架构状态和 signal 规则；不能只重放任意陈旧用户指针。

## 调用号

`number.rs` 的常量以 Linux asm-generic 64 为准。部分传统调用在该 ABI 没有独立编号，使用 `usize::MAX` 哨兵，例如 POLL/SELECT/EPOLL_WAIT；用户态应调用 ppoll/pselect6/epoll_pwait。哨兵绝不能用于分发表数组下标。

新增常量只表示“ABI 知道这个号”，不表示内核实现。实际可用性以 impl-kernel `ARG_SYSCALL_TABLE`/`SPECIAL_SYSCALL_TABLE` 为准；空槽和超范围统一 ENOSYS。

不要为测试随意分配私有低号，这会和 Linux 后续 ABI 冲突。架构专用调用（如 RISC-V hwprobe/flush_icache）应保留官方号，并在其它架构返回 ENOSYS。

## errno 和返回值

`ErrNo::from_raw` 只接受正数；`raw()` 仍为正，`user_ret()` 才取负。领域组件返回自己的错误，syscall handler 映射成 `KernelResult<T> = Result<T, ErrNo>`，最后统一构造 `UserRet`。

```text
VFS/MM/task error -> ErrNo(正值) -> UserRet::from_error -> -errno -> 返回寄存器
成功 usize       ----------------> UserRet::from_success -> 非负值
```

严禁双重取负：不要把 `-EFAULT` 传给 `ErrNo::from_raw`，也不要对 `UserRet.0` 再取负。`from_success(v)` 直接 `v as isize`，没有检查 `v <= isize::MAX`；handler 返回地址/长度前必须保证用户 ABI 可表示，否则“大成功值”会看起来像错误。

partial success 遵循具体 Linux syscall：若已传输字节后才 fault/signal，通常返回正的已完成数，而不是 EFAULT/EINTR。这个策略不能由 `UserRet` 自动判断。

## 新 syscall 完整实例

以新增 `foo(fd, user_buf, len, flags)` 为例：

1. 在 `number.rs` 加官方 asm-generic 号，确认不与现有常量重复且小于分发表设计上限。
2. 如有用户结构，结构放 impl-kernel 并写 `repr(C)`/size/offset 断言；不要让 API crate 依赖业务对象。
3. handler 从 `args.arg(0..3)` 解析，先检查 flags/长度，再用统一 user-copy。
4. 领域错误映射正 `ErrNo`，以 `UserRet::from_kernel_result` 或显式构造结束。
5. 在 `syscall_nr_dispatch.rs` 的普通表或特殊表登记。
6. 若 Linux 允许 SA_RESTART，审计 partial side effect 后再加入 restart 白名单。
7. 增加未知号、坏指针、零长、边界 flags、partial 与双架构直调测试。

## 回归清单

- `ErrNo::from_raw(0/-1)` 为 None，所有常量 raw 为正、user_ret 为负；
- success 0、最大合法 `isize`，拒绝超过范围的 handler 结果；
- SyscallArgs 寄存器顺序和 MAX 参数边界；
- 官方编号与目标 Linux headers 对照，无重复；
- `usize::MAX` 哨兵永不索引 table；
- 已编号未登记返回 ENOSYS，架构专用调用在错误架构返回 ENOSYS；
- 每个新增 syscall 在 RISC-V 与 LoongArch 用户态直接 `ecall/syscall` 验证。
