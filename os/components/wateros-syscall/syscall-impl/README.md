# syscall-impl

[返回 syscall 总览](../README.md)

`syscall-impl` 存放 ABI 到内核对象之间的适配实现。目前唯一实现是
`impl-kernel/`：它使用 WaterOS 的 task、VFS、MM、IPC、network 和 platform
能力完成 Linux 兼容语义。

实现 crate 可以依赖内核组件，但不能把 Linux 用户指针、裸 flags 或 errno 继续
下放到通用组件。通用组件返回领域错误和稳定对象，syscall 层负责：

1. 解析 ABI 与校验未知 flags；
2. 安全复制用户数据；
3. 调用内核能力且不跨锁调度；
4. 把领域错误映射为 Linux errno；
5. 对部分成功、`EFAULT` 回滚和 `EINTR` 重启负责。

详见 [impl-kernel 文档](impl-kernel/README.md)。
