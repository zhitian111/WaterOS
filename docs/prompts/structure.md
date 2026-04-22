# 项目结构 Prompt

本文件描述 WaterOS 当前文本文档与核心工程的结构，用于帮助 Agent 在修改文件前快速判断上下文和同步范围。

## 仓库主结构

### `/home/zhitian/project/WaterOS_refactor/docs`

文档主目录，包含 Agent prompt、重复任务、导出结果、协作指南、路线图与架构快照。

### `/home/zhitian/project/WaterOS_refactor/os`

内核主工程。根 crate `wateros` 聚合各一级组件，并在 `src/main.rs` 中组织启动、自检、驱动初始化、中断与计时器流程。

#### `/home/zhitian/project/WaterOS_refactor/os/components`

内核一级组件集合。当前主要组件包括：

##### `wateros-abi`

ABI、系统调用和用户内核约定相关组件。

##### `wateros-base`

基础类型、地址表示与通用配置。

##### `wateros-driver`

驱动总入口，继续拆分为 block、character、network 与平台实现。

##### `wateros-fs`

文件系统组件，当前已有 `impl-devfs` 与 `impl-dummy`。

##### `wateros-ipc`

IPC 总入口，继续拆分为 pipe、signal、futex、event、shm、waitqueue。

##### `wateros-mm`

内存管理组件，包含 `mm-api`、`mm-impl`、`mm-frame-alloctor`。

##### `wateros-platform`

平台组件，继续拆分为 platform-api、arch、firmware 和 platform-impl。

##### `wateros-runtime`

运行时组件，继续拆分为 console、logging、panic、heap-allocator。

##### `wateros-task`

任务与调度相关组件。一级组件根 crate 对外聚合任务对象与调度入口，当前上层调用主要通过 `init`、`spawn_kernel_task`、`run_first_task`、`yield_now`、`schedule_tick`、`current_task_id` 等接口进入。

其中 `task-api/api-v0/` 定义 `TaskId`、`TaskContext`、`KernelTask` 等基础契约，`task-scheduler/` 继续拆分 `scheduler-api/api-v0/` 与 `scheduler-impl/impl-dummy/`；`task-impl/impl-dummy/` 当前仍偏占位，实际可运行的轮转调度逻辑主要位于调度器 impl 中。

修改本组件时，除检查组件根 `Cargo.toml` 与 `src/lib.rs` 外，还应同步关注 `os/src/main.rs` 的启动接线，以及平台中断/定时器路径对 `task::schedule_tick()` 的调用关系。

##### `wateros-utils`

通用工具组件。

##### `wateros-vfs`

虚拟文件系统组件。

### `/home/zhitian/project/WaterOS_refactor/user`

用户态工程，包含示例和测试程序，可作为内核接口开发时的重要验证侧来源。

## 一级组件的固定模式

大多数组件遵循以下组织方式：

- 一级组件根 crate：统一 feature、统一导出、统一对外入口。
- `*-api/api-v0/`：定义 trait、类型、错误与常量。
- `*-impl/impl-*/`：实现某个具体平台或算法。
- `src/lib.rs`：聚合导出层，根据 feature 选择具体实现。

## 重要同步文件

修改以下内容时，Agent 应主动检查这些文件是否需要同步更新：

- `os/Cargo.toml`
- `os/src/main.rs`
- `os/feature-tree.txt`
- `os/Makefile`
- 各一级组件 `Cargo.toml`
- 各一级组件聚合 `src/lib.rs`
- `docs/guides/workflow.md`
- `docs/guides/documentation.md`
- `docs/guides/versioning.md`
- `docs/roadmap/todolist.md`
- `docs/architecture/snapshot.md`
- `docs/exports/`

## 修改时的同步判断

- 如果新增或切换 feature，需要同步检查根 crate 和对应组件的 `Cargo.toml`。
- 如果修改聚合接口，需要同步更新 `docs/exports/public-api/`。
- 如果新增 impl，需要同步更新 `docs/exports/impl-guide/`、`docs/guides/task-board.md` 和 `docs/roadmap/todolist.md`。
- 如果某组件能力明显变化，需要同步更新 `docs/exports/features/` 和 `docs/architecture/snapshot.md`。
