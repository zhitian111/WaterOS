# WaterOS TodoList

本文件用于维护阶段性目标、模块开发计划和后续新增任务入口。它不是单次任务记录，而是面向当前阶段目标的持续计划表。

## 当前阶段目标

- 在 QEMU riscv64 + OpenSBI 路径上持续推进内核 bring-up。
- 在 QEMU loongarch64 virt 路径上维持 boot/task bring-up 可回归。
- 逐步把各一级组件从骨架或占位实现推进到可用实现。
- 维持 API/impl 分层、feature 选择链和聚合导出链的稳定性。
- 持续刷新文档、架构图、公共 API 与功能快照。

## 一级组件推进情况

| 组件 | 当前状态 | 下一步重点 |
|------|----------|------------|
| wateros-platform | 已形成较稳定的 API/impl/聚合模式；LoongArch64 virt 已具备 UART、时间频率和 CSR timer 初步实现 | 继续补齐 LoongArch reset、paging 与平台能力文档 |
| wateros-driver | 已有 QEMU RISC-V OpenSBI 实现路径 | 继续补充驱动实现和公共 API 说明 |
| wateros-mm | 已具备 `impl-sv39` 和帧分配组件 | 继续收敛地址空间与映射接口 |
| wateros-runtime | 已有 console、logging、panic、heap allocator | 补齐对外能力文档和注释 |
| wateros-fs | 仍处于早期推进阶段 | 持续完善 `impl-devfs` |
| wateros-ipc | 部分子组件仍为占位实现 | 优先推进 pipe、signal、waitqueue 相关能力 |
| wateros-task | 已能在 RISC-V 与 LoongArch64 路径接入 round-robin kernel task 调度 | 继续验证 trap 驱动抢占、用户任务恢复与等待队列 |
| wateros-vfs | 仍偏骨架 | 与 fs、ipc、task 的资源模型对齐 |
| wateros-abi | 有基础结构；Linux generic 64-bit syscall 表已供 RISC-V/LoongArch64 早期复用 | 收敛 syscall 与用户态约定 |
| wateros-base | 作为基础依赖存在 | 继续承接地址与基础类型 |
| wateros-utils | 当前作为通用工具层 | 保持轻量，不承载跨层耦合 |

## 当前优先任务

- 完善 `wateros-fs` 与 `wateros-vfs` 的文档和能力边界。
- 梳理 `wateros-ipc`、`wateros-task` 仍为骨架的部分。
- 推进 LoongArch64 的 paging、driver 与用户态 syscall 验证，避免直接复用 RISC-V Sv39/设备实现。
- 持续导出每个组件的公共 API、实现指南和功能快照。
- 在新增 impl 时同步维护 `docs/guides/task-board.md`。

## 新增任务入口

新增任务时请至少补充以下信息：

- 目标组件
- 任务类型：设计、实现、文档、重构、验证
- 是否依赖某个 `api-v0`
- 是否需要新增 `impl-*`
- 预计同步更新哪些文档
