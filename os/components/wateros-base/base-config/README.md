# wateros-base-config

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-base](../README.md)

`wateros-base-config` 是无状态、`no_std` 的编译期配置 crate。它解决多个组件重复
声明同一个常量的问题，不负责读取 DTB、命令行或运行时 CPU/内存状态。

## 配置模块

| 模块 | 配置内容 | 主要消费者 |
|---|---|---|
| `fs` | 文件页大小、页缓存容量、预取和块缓存 | VFS、FS、块设备缓存 |
| `ipc` | pipe 默认容量 | IPC pipe |
| `klog` | 消息槽、正文环和单条日志上限 | klog |
| `mm` | 内核堆、QEMU virt RAM/MMIO 回退布局 | runtime allocator、MM |
| `syscall` | syscall 参数槽数量 | syscall API、trap 参数包 |
| `task` | CPU 容量、tick、栈、nice 权重 | task、scheduler、platform |

## 放置规则

适合放在这里的值必须同时满足：

1. 编译期固定；
2. 被多个底层组件共享，或属于组件装配所需的全局容量；
3. 不依赖探测到的硬件状态；
4. 不包含运行时可变策略。

例如 `MAX_CPUS` 是静态数组容量，不是当前 configured/online CPU 数；
`QEMU_VIRT_PHYS_RAM_END` 是 DTB 不可用时的回退值，不应覆盖固件实际报告的内存；
`NICE_TO_WEIGHT` 只参与 SCHED_OTHER 的 vruntime 换算，不决定 FIFO/RR 优先级。

新增常量时应在定义处说明单位、区间是否包含上界、`0` 的特殊含义以及消费者。
删除或改名时先用 `rg` 检查全部架构和 feature，避免只修复默认 profile。

## 消费流程、并发与生命周期

配置调用链是“Cargo feature/target 选择常量 → 各 crate 编译进静态容量 → bring-up 再用 DTB/固件值限制运行集合”。常量无运行期锁和可变生命周期，但它决定静态数组、heap/cache预留和ABI布局；多个CPU只读同一编译结果。

配置错误通常不会在定义处失败，而会表现为启动越界、heap OOM、CPU mask错误或缓存压力。修改后用 `rg` 列出所有消费者，检查单位和checked算术，分别运行 RV/LA `make check`，并对容量边界做运行回归；RAM/CPU等硬件值还要验证运行期探测优先于fallback。
