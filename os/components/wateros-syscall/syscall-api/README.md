# syscall-api

[返回 syscall 总览](../README.md)

`syscall-api` 保存跨 trap、内核实现和测试代码共用的稳定 ABI。当前版本位于
`api-v0/`，核心内容是 Linux asm-generic 64 位调用号、正值 `ErrNo`、参数包
`SyscallArgs` 和最终返回包装 `UserRet`。

## 约束

- 本层是 `no_std` 数据契约，不得依赖 task、VFS、MM 或具体架构实现。
- `number.rs` 登记编号不代表已经实现；是否可调用以 `syscall_nr_dispatch.rs` 为准。
- handler 内部传递正 errno，只在构造 `UserRet` 时编码为 `-errno`。
- 新增 syscall 时先补编号和 ABI 结构断言，再在 impl-kernel 分发和实现。

版本字段、文件职责与扩展方法见 [api-v0 文档](api-v0/README.md)。
