# wateros-platform 公共 API 快照

## 当前定位

聚合层当前导出 `boot`、`arch`、`time`、`timer`、`reset`、`console`、`interrupt` 等能力，并通过 feature 绑定 `impl-qemu-riscv64-opensbi`。这是最典型的 API/impl/聚合范式实现。

其中 `platform-arch` 现在还承担了架构级任务切换上下文抽象 `ArchTaskContext` 及当前架构具体实现的组织工作，用于表达“当前 CPU 架构下任务切换最小需要保存的寄存器集合”。`TaskContext` 已不再属于 `wateros-task` 公共 API，而是作为机制层实现细节由 `task-impl` / `task-scheduler` 消费。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。
