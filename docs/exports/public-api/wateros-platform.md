# wateros-platform 公共 API 快照

## 当前定位

聚合层当前导出 `boot`、`arch`、`time`、`timer`、`reset`、`console`、`interrupt` 等能力，并通过 feature 绑定 `impl-qemu-riscv64-opensbi`。这是最典型的 API/impl/聚合范式实现。

其中 `platform-arch` 现在还承担了架构级任务切换上下文抽象 `ArchTaskContext` 及当前架构具体实现的组织工作，用于表达“当前 CPU 架构下任务切换最小需要保存的寄存器集合”。Stage3A 之后，arch 侧的 `goto_task_entry(...)` 语义已进一步收敛为“向任务 runtime 传递 opaque bootstrap 指针”，而不再把 task 启动协议对象暴露到公共 API。`TaskContext` 也继续只作为 `task-impl` / `task-scheduler` 的机制层细节被消费。当前 trap 抽象还额外提供了 `user_sp`、`returns_to_user`、`set_user_sp`、`set_return_to_user`、`prepare_user_return` 这类接口，用来明确表达“这次 trap 恢复将返回到用户态”的语义，为后续 user task 入口与 user trap return 链路预留统一接入面。RISC-V arch 实现现已补出最小的 user-task 入口 trampoline 与 `__wateros_arch_restore_user_task(...)` 恢复桩，可从任务对象保存的 trap frame 直接走一次 `sret` 首次进入用户态。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
