# WaterOS 第一阶段任务切换实现整理

本文件整理本轮围绕“内核态任务切换 + 定时器驱动调度”所做的相关改动，目标是记录当前阶段已经打通的主线、改动位置、验证结果与后续边界。

## 目标

本阶段目标是在保留 WaterOS 组件化边界的前提下，完成：

- `qemu-riscv64-opensbi` 下的内核态任务创建与切换
- 基于 trap/timer 的最小 round-robin 调度
- 启动后创建两个演示任务并切入执行
- 保持现有 console/logging/mm/driver 自检主线不回归

本阶段明确不做：

- 用户态任务
- 进程/线程分离
- syscall/process/fs/ipc/vfs 完整接入
- 用户地址空间与 TrapContext 归属到任务对象

## 主要改动概览

### 1. 根 crate 接入 `wateros-task`

为了让任务系统正式进入主启动流，根 crate 新增了对 `wateros-task` 的依赖：

- `os/Cargo.toml`

这样 `os/src/main.rs` 可以直接调用：

- `task::init()`
- `task::spawn_kernel_task(...)`
- `task::yield_now()`
- `task::run_first_task()`

### 2. `wateros-task` 从空壳变为 facade

`wateros-task` 原先基本是占位代码，这一轮改成了面向第一阶段任务系统的聚合 facade：

- `os/components/wateros-task/src/lib.rs`

当前对外暴露的核心能力：

- `TaskId`
- `TaskStatus`
- `TaskContext`
- `KernelTask`
- `spawn_kernel_task`
- `run_first_task`
- `yield_now`
- `schedule_tick`
- `current_task_id`

同时保留了组件边界，实际调度逻辑继续下沉到 `task-scheduler`。

### 3. 补齐 `task-api` 与 `scheduler-api`

为了先固定接口、再填实现，这一轮先给两个 API crate 定了第一阶段可用接口。

`task-api`：

- `os/components/wateros-task/task-api/api-v0/src/lib.rs`

新增内容：

- `TaskId = usize`
- `KernelTaskEntry = extern "C" fn(usize) -> !`
- `TaskStatus::{Ready, Running}`
- `TaskContext { ra, sp, s[12] }`
- `TaskContext::zero_init()`
- `TaskContext::goto_entry(...)`
- `KernelTask`
- `IDLE_TASK_ID`

`scheduler-api`：

- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/lib.rs`

定义了第一阶段 `Scheduler` trait，覆盖：

- `init`
- `spawn_kernel_task`
- `run_first_task`
- `suspend_current_and_run_next`
- `schedule_tick`
- `current_task_id`

## 调度器实现

### 1. 最小 round-robin 调度器

主要实现位于：

- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-dummy/src/lib.rs`

当前实现采用：

- 单核模型
- `VecDeque` 就绪队列
- 一个显式 `idle task`
- 每个任务一块独立内核栈
- `TaskContext` 只保存 `ra/sp/s0-s11`

调度器内部维护：

- `current`
- `ready_queue`
- `idle_task`
- `bootstrap_task_cx`
- `next_task_id`

### 2. 任务栈与入口

每个任务分配固定大小的内核栈，当前使用：

- `32 KiB` 对齐内核栈

任务初始上下文通过：

- `TaskContext::goto_entry(__task_entry, kstack_top)`

建立，再由汇编入口 stub 把：

- `s0` 作为任务入口函数地址
- `s1` 作为任务参数

转发给 Rust 侧的 `__wateros_task_entry(...)`。

### 3. 独立上下文切换汇编

任务切换汇编独立实现为：

- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-dummy/src/switch.S`

包含两个符号：

- `__switch`
- `__task_entry`

其中 `__switch` 只负责保存与恢复：

- `ra`
- `sp`
- `s0..s11`

这部分实现与 trap 保存逻辑完全分离，符合第一阶段“任务上下文”和“trap 上下文”分治的设计。

### 4. 借用冲突修复

第一次运行时，调度器在 `RefCell` 借用尚未释放时执行上下文切换，导致后续再次进入调度器时触发：

- `RefCell already borrowed`

修复方式是把真正的 `__switch(...)` 调用移到 `with_scheduler(...)` 借用结束之后，使切换发生在调度器可重入状态下。

## trap / timer 改动

### 1. trap Rust 入口不再自旋

主要改动位于：

- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm`

此前 trap 入口在处理后会停在死循环里；现在改成：

- timer interrupt 到来
- 重新设置下一次 timer deadline
- 调用 `task::schedule_tick()`
- 返回 trap 汇编
- 恢复现场并 `sret`

### 2. TrapContext 与 TaskContext 分离

当前实现明确区分两类上下文：

- `TrapContext`
  负责一次 trap 的现场保存与 `sret` 返回
- `TaskContext`
  负责任务间的上下文切换

这是第一阶段很重要的边界，避免把“trap 恢复语义”和“任务调度语义”混在同一结构里。

### 3. trap 汇编恢复链路

`trap.asm` 当前流程为：

1. 在当前栈上分配 `TrapContext`
2. 保存通用寄存器和控制寄存器
3. 调用 `trap_entry_rust(cx_ptr)`
4. 从当前 `sp` 指向的 `TrapContext` 恢复寄存器
5. 执行 `sret`

这意味着：

- 如果 timer 中断里发生了任务切换，最终仍会回到“当前被选中任务”的 trap 恢复路径
- trap 本身不负责调度器状态管理，只负责保存与恢复现场

### 4. timer 重装与可观测 trace

为了确认定时器链路正在持续工作，`trap.rs` 中加入了轻量 trace：

- 每累计若干次 timer tick 打印一次 `"[trap] timer tick N"`

这能帮助确认：

- timer 中断持续触发
- trap 没有卡死
- 调度并不只依赖 cooperative `yield`

## 启动流改动

主要改动位于：

- `os/src/main.rs`

当前 `kernel_main` 的关键顺序为：

1. 解析 boot 参数
2. 初始化 driver、console、logging、heap
3. 初始化 arch/trap
4. 执行 MM / frame allocator / Sv39 自检
5. 执行 driver 自检
6. 初始化 task scheduler
7. 创建两个演示性内核任务
8. 开启 timer source
9. 设置首次 timer deadline
10. 开启全局中断
11. 调用 `task::run_first_task()`

演示任务为：

- `demo_task_a`
- `demo_task_b`

它们都运行在 S-mode，通过忙等 + `task::yield_now()` 周期性打印：

- `task A tick N`
- `task B tick N`

## 验证结果

本轮已经完成的验证包括：

### 构建验证

- `cargo check --manifest-path os/Cargo.toml --target riscv64gc-unknown-none-elf`
- `cargo build --manifest-path os/Cargo.toml --target riscv64gc-unknown-none-elf --release`

两者均已通过。

### QEMU 短时运行验证

已使用短时 smoke test 验证：

- 能进入 `kernel_main`
- console/logging 正常输出
- MM 与 driver 自检不回归
- 两个内核任务能持续交替打印
- trap 中可观察到 `timer tick` trace

运行现象说明当前已经打通：

- 启动入口
- trap 恢复链路
- timer 重装
- 任务切换
- cooperative yield 与 timer 驱动调度共存

## 当前限制

本阶段仍有这些限制：

- 只支持单核
- 只支持内核态任务
- 没有阻塞队列、睡眠队列、退出语义
- 没有用户态 trap 返回路径
- 没有把 TrapContext 纳入任务对象生命周期
- 没有实现进程/线程、信号、wait、syscall 调度协作

## 后续建议

如果继续推进第二阶段，建议按这个顺序往下接：

1. 给任务对象引入更明确的生命周期与退出语义
2. 把 timer-preemptive 调度与显式 `yield` 的统计/策略分离
3. 为任务补 trap frame 归属关系
4. 引入用户栈与用户入口
5. 再接 syscall / process / fs 主线

## 相关文件

本轮最核心的改动文件如下：

- `os/Cargo.toml`
- `os/src/main.rs`
- `os/components/wateros-task/src/lib.rs`
- `os/components/wateros-task/task-api/api-v0/src/lib.rs`
- `os/components/wateros-task/task-scheduler/src/lib.rs`
- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/lib.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-dummy/src/lib.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-dummy/src/switch.S`
- `os/components/wateros-platform/platform-arch/src/lib.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/trap.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm`
