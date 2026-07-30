# wateros-syscall

`wateros-syscall` 统一维护用户态、trap 层和内核 handler 之间的 syscall 契约与实现。
原 `wateros-abi` 只包含 syscall 相关类型，现已并入 `syscall-api/api-v0`，避免调用号
和参数/返回编码分散在两个组件中。

## 结构

```text
wateros-syscall/
├── src/lib.rs                       # facade：重导出 API 和内核入口
├── syscall-api/api-v0/
│   ├── args.rs                      # SyscallArgs / SyscallPacket
│   ├── errno.rs                     # 正 errno 与 KernelResult
│   ├── number.rs                    # 调用号常量与 SyscallNumber
│   └── return_value.rs              # UserRet
└── syscall-impl/impl-kernel/
    ├── syscall_nr_dispatch.rs       # 调用号到 handler 的分发
    └── sys/                         # fs、task、ipc、net、time 等具体语义
```

`syscall-api-v0` 是纯 `no_std` 数据契约，不依赖 platform、task、MM 或内核实现。
`impl-kernel` 可以依赖这些服务并实现具体语义。

## 关键约定

| 类型 | 内核内部表示 | 跨用户态边界时的表示 |
| --- | --- | --- |
| `ErrNo` | 正数 Linux errno | 不能直接作为 syscall 返回值 |
| `KernelResult<T>` | `Result<T, ErrNo>` | 尚未编码 |
| `UserRet` | 单个 `isize` | 成功为非负值，错误为 `-errno` |
| `SyscallArgs` | 固定数量 `usize` 槽位 | 槽位顺序必须与目标架构的参数寄存器一致 |
| `SyscallNumber` | 裸调用号的透明包装 | 不保证该调用号已由内核实现 |

因此 handler 应在最终返回到陷阱层前使用 `UserRet::from_success`、
`UserRet::from_error` 或 `UserRet::from_kernel_result`。不要在中间层把
`ErrNo` 预先取负，否则容易发生双重编码。

## Feature 与依赖方向

- `api-v0`：导出当前 syscall API 版本。
- `impl-kernel`：启用内核 syscall 分发和实现，同时启用 `api-v0`。

允许的核心依赖方向是：

```text
platform-arch-api ──> syscall-api-v0
task-impl-core ─────> syscall-api-v0
syscall-impl-kernel ─> syscall-api-v0 + platform/task/mm/...
```

`syscall-api-v0` 禁止反向依赖以上组件，否则会产生依赖环。

## 边界

- 调用号、errno、参数和返回编码：`wateros-syscall-api-v0`。
- 陷阱帧、参数寄存器的读取/写回：`wateros-platform-arch-api-v0` 和 arch impl。
- syscall 的业务实现、用户内存访问和子系统错误映射：`impl-kernel`。
