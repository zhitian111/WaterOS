# WaterOS 组件架构文档

[项目首页](../../README.md) · [文档总览](../README.md) · [内核工程](../../os/README.md) · [架构图](../assert/diagrams/wateros-architecture.svg)

本页是 WaterOS 一级内核组件架构 README 的统一验收入口。文档仍保存在各组件
源码旁，以使路径、Cargo feature 和实现引用与代码保持同步；本页不复制其内容，避免
形成两份会漂移的技术事实。所有链接均指向当前权威 README。

组件文档采用同一验收重点：职责边界、代码地图、关键状态、真实端到端链路、并发与
资源生命周期、初始化/feature/可观测性，以及当前限制。派发或补写任务时使用
[`ARCHITECTURE_DOCUMENTATION_PROMPTS.md`](../../os/components/ARCHITECTURE_DOCUMENTATION_PROMPTS.md)。

## 基础与运行环境

| 组件 | 当前架构文档 | 主要边界 |
| --- | --- | --- |
| `wateros-base` | [README](../../os/components/wateros-base/README.md) | 基础类型、per-CPU 数据与低层同步原语。 |
| `wateros-platform` | [README](../../os/components/wateros-platform/README.md) | ISA、固件、QEMU 板级环境、Trap 与 SMP。 |
| `wateros-runtime` | [README](../../os/components/wateros-runtime/README.md) | 控制台、日志、堆分配器、串口与 panic。 |
| `wateros-debug` | [README](../../os/components/wateros-debug/README.md) | GDB 可观测状态与低层调试 ABI。 |
| `wateros-driver` | [README](../../os/components/wateros-driver/README.md) | 设备发现、注册与 VirtIO 传输。 |

## 核心内核服务

| 组件 | 当前架构文档 | 主要边界 |
| --- | --- | --- |
| `wateros-mm` | [README](../../os/components/wateros-mm/README.md) | 物理页、地址空间、页表与用户内存访问。 |
| `wateros-task` | [README](../../os/components/wateros-task/README.md) | 进程/线程生命周期、调度与上下文切换。 |
| `wateros-ipc` | [README](../../os/components/wateros-ipc/README.md) | 管道、futex、信号、共享内存和任务等待适配。 |
| `wateros-cred` | [README](../../os/components/wateros-cred/README.md) | 任务凭证侧表与生命周期钩子。 |
| `wateros-klog` | [README](../../os/components/wateros-klog/README.md) | 内核保留日志的并发环形缓冲区。 |

## I/O、接口与交互

| 组件 | 当前架构文档 | 主要边界 |
| --- | --- | --- |
| `wateros-fs` | [README](../../os/components/wateros-fs/README.md) | 根卷、文件系统后端和块 I/O 适配。 |
| `wateros-vfs` | [README](../../os/components/wateros-vfs/README.md) | fd 会话、路径、挂载和页缓存一致性。 |
| `wateros-network` | [README](../../os/components/wateros-network/README.md) | smoltcp 运行时、网络设备适配与 socket 后端。 |
| `wateros-tty` | [README](../../os/components/wateros-tty/README.md) | 终端、PTY 和行规程状态。 |
| `wateros-gui` | [README](../../os/components/wateros-gui/README.md) | 内核 GUI 合成、输入事件和 framebuffer 输出。 |
| `wateros-syscall` | [README](../../os/components/wateros-syscall/README.md) | 用户/内核事务、ABI 分发与用户内存拷贝。 |
| `wateros-utils` | [README](../../os/components/wateros-utils/README.md) | 无全局状态的纯工具与表格格式化。 |

## 验收顺序

从 `wateros-base`、`wateros-platform`、`wateros-runtime` 开始确认启动与底层约束，随后阅读
`wateros-mm`、`wateros-task` 和 `wateros-ipc` 的状态生命周期。I/O 路径应按
`wateros-driver` → `wateros-fs` → `wateros-vfs` → `wateros-syscall` 串联核对；网络、TTY
和 GUI 文档用于补足其各自的设备与用户交互边界。

每份 README 是当前源码的说明，而不是已验证能力的承诺。验收时应以其中引用的路径、符号、
feature 条件与实际构建结果为准。
