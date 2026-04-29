# wateros-task 后续改进路线

## 用途

本文件记录 `wateros-task` 从当前 bring-up 可用状态继续推进到稳定内核任务系统的阶段性路线。它用于后续实现、评审和文档同步时对齐目标，不作为已经完成能力的声明。

## 事实来源

- `os/components/wateros-task/src/lib.rs`
- `os/components/wateros-task/task-api/api-v0/`
- `os/components/wateros-task/task-impl/impl-core/`
- `os/components/wateros-task/task-scheduler/`
- `os/components/wateros-platform/platform-arch/arch-api/api-v0/src/trap.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs`
- `os/src/self_tests/task.rs`
- `docs/exports/public-api/wateros-task.md`
- `docs/exports/features/wateros-task.md`
- `docs/exports/public-api/wateros-platform.md`

## 当前状态摘要

`wateros-task` 当前已具备最小任务系统主线：内核任务创建、idle 任务、round-robin 调度、timer tick 调度、主动让出、阻塞、睡眠、通用等待、任务退出等待、zombie 回收和最小用户任务骨架。

当前 task/runtime/trap 路径已经开始收敛到 `platform-arch`：task 机制层通过 `ActiveTrapFrame`、`ArchTrapFrame`、`TrapContextRead` 和 `TrapContextWrite` 这类架构层契约保存和恢复 trap 现场，task 公共 API 则通过 `TaskTrapSnapshot` 暴露架构无关的 trap 语义快照。后续重点是继续把这些 bring-up 能力整理成稳定 API、明确错误边界，并补齐真实用户任务所需的地址空间、loader 和异常处理链路。

## 阶段路线

| 阶段 | 目标 | 主要改动 | 验收标准 | 关联位置 |
|------|------|----------|----------|----------|
| 阶段 1：trap frame 归属收敛 | 让 task 公共 API 保持架构无关，机制层继续依赖架构 trap frame 契约 | 继续确认 `TaskTrapSnapshot` 的字段是否足够；补齐 `ArchTrapFrame` 文档；保持完整 `ActiveTrapFrame` 只在 impl/runtime/scheduler 机制层流转 | task 侧没有新的 RISC-V trap frame 布局副本；公共文档明确真实布局归属 `platform-arch`；现有用户任务自检继续通过 | `platform-arch/arch-api/api-v0/src/trap.rs`、`task-api/api-v0/src/trap_frame.rs`、`task-impl/impl-core/src/tcb.rs` |
| 阶段 2：task/scheduler API 边界完善 | 把当前 panic/占位路径推进为稳定错误模型和任务句柄模型 | 设计 `TaskError`、`TaskResult`；为无效 `TaskId`、无效 wait queue、资源不足提供非 panic 返回路径；设计 `TaskHandle` 或 generation 语义，避免任务号复用后的悬空等待和误回收 | 新增 API 能表达失败原因；scheduler 内部对外可达路径不因无效输入直接 panic；reap/wait 文档写清 task id 或 handle 的生命周期 | `task-api/api-v0`、`task-scheduler/scheduler-api/api-v0`、`task-scheduler/scheduler-impl/impl-round-robin` |
| 阶段 3：真实用户任务主线 | 从最小用户任务骨架推进到可加载、可隔离、可处理异常的用户任务 | 接入 `wateros-mm` 地址空间；把用户栈映射到用户地址空间并设置权限；规划用户镜像 loader；在进入/返回用户态时处理 satp 切换；补齐 copy in/out；定义 page fault 后退出或错误处理策略 | 用户任务不再依赖内核堆地址作为用户栈长期模型；最小用户程序可通过 loader 创建并执行 syscall；用户态 fault 有确定退出或错误路径 | `task-api/api-v0/src/user.rs`、`task-impl/impl-core/src/tcb.rs`、`platform-arch/arch-impl/impl-riscv64/src/trap.rs`、`wateros-mm` |
| 阶段 4：等待与退出模型完善 | 把当前 wait handle 和 zombie 回收推进为更通用的阻塞对象和进程式退出语义 | 泛化 wait handle 到 block object 或统一等待对象；定义 waitpid 风格父子关系；写清 zombie 生命周期、重复 reap、等待已退出任务、等待不存在任务等规则；与 IPC waitqueue/futex/event 复用同一等待模型 | wait、timeout、wake、exit、reap 的边界行为可由 API 文档解释；IPC 同步原语可复用 task 等待机制；相关自检覆盖正常唤醒、超时、已退出目标和无效目标 | `task-api/api-v0/src/wait.rs`、`task-scheduler/scheduler-impl/impl-round-robin/src/queues.rs`、`wateros-ipc/ipc-waitqueue` |
| 阶段 5：验证与文档补齐 | 建立 task 组件稳定自检入口，并让文档随能力变化同步更新 | 在 task 聚合层规划统一自检入口，减少 `os/src/main.rs` 对 self test 细节的直接依赖；整理 QEMU 自检场景；同步更新公共 API、功能快照、架构快照和任务板 | `cargo check --release --target riscv64gc-unknown-none-elf --features qemu-riscv64-opensbi` 通过；QEMU 日志可观察 task 阶段自检；相关 docs 只描述真实落地能力和明确 TODO | `os/src/self_tests/task.rs`、`os/src/main.rs`、`docs/exports/public-api/wateros-task.md`、`docs/exports/features/wateros-task.md` |

## 验收标准

- 每个阶段完成时都能说明影响的是聚合层、API 层、impl 层、scheduler 层、platform-arch 层或文档层中的哪些部分。
- 新增或改变公共接口时，同步检查 `Cargo.toml` feature 传递、聚合导出、公共 API 文档和功能快照。
- 涉及 trap、地址空间、用户栈、satp、page fault 的实现必须明确架构假设和异常路径。
- 涉及 wait、wake、exit、reap 的实现必须覆盖正常路径、超时路径、无效目标和重复操作。
- 阶段完成后至少运行 RISC-V 目标 `cargo check`；涉及运行时行为时补充 QEMU self-test 观察点。

## 后续同步入口

- `docs/roadmap/todolist.md`：阶段目标或组件状态发生变化时更新。
- `docs/guides/task-board.md`：任务被拆分、认领或进入 review 时更新。
- `docs/exports/public-api/wateros-task.md`：聚合层导出或 API 语义变化时更新。
- `docs/exports/features/wateros-task.md`：功能状态从 TODO 变为已落地时更新。
- `docs/exports/public-api/wateros-platform.md`：trap、task context 或架构聚合导出变化时更新。
- `docs/architecture/snapshot.md`：task 与 platform/mm/ipc 的关系发生架构级变化时更新。
