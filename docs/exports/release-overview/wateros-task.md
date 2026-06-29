# wateros-task — 版本概述

## 定位

`wateros-task` 是 WaterOS 内核的 **任务与进程子系统**：在单核 bring-up 阶段提供可运行的多任务环境，支撑用户态程序、syscall、信号与 LTP 子集测试。

本组件回答三个问题：

1. **谁在下一次运行**（调度器 + 就绪/等待队列）
2. **每个任务长什么样**（TCB、内核栈、用户 trap 现场）
3. **用户看到的进程/线程是谁**（PID/TID registry，与 wait/clone/fork 语义对齐）

## 当前阶段目标（已达成）

- 内核线程与用户进程/线程的创建、切换、退出与回收
- `fork` / `clone` / `execve` 与内存子系统协作的基本路径
- 阻塞、睡眠、超时等待、wait queue 同步原语
- 进程组、会话、stopped/continued、rlimit/nice 等 syscall 所需 registry 状态
- QEMU virt 上单核稳定运行（RISC-V Sv39 / LoongArch64 经 platform-arch 抽象）

## 适用范围

| 场景 | 支持程度 |
|------|----------|
| bring-up / busybox / LTP 单核测试 | 主线 |
| 多核 SMP | 未支持（UP 假设） |
| 完整 CFS/RT 调度 | 部分（API 与队列骨架；有效策略多为 OTHER） |
| 完整 VFS/信号/ cred 生命周期 | 由上层 syscall 与占位句柄协作，非 task 内建 |

## 对外承诺（稳定面）

通过根 `task::` 模块导出的函数与 `api-v0` 类型是 syscall、trap、MM bring-up 的 **集成契约**。更换调度算法（`impl-multi-class` ↔ `impl-round-robin`）或调整 TCB 布局时，应保持：

- `TaskId` / `TaskWaitHandle` / `ProcessDescriptor` 语义不变
- `begin_current_trap_frame_access` / `restore_current_trap_frame` 与 arch trap handler 的调用约定不变
- C ABI 运行时符号名不变（`__wateros_task_runtime_*`）

## 已知限制（使用者须知）

- **单 CPU 亲和性**：`sched_setaffinity` 仅验证 CPU0。
- **exec 多线程**：保守要求 leader 发起；其它线程由 `terminate_other_threads_for_exec` 清理。
- **测试清理**：脚本结束应调用 `purge_all_user_processes`，避免 fork 孤儿泄漏页帧。
- **僵尸进程**：需显式 `reap_*`；registry 保留 Exited 记录直至 reap。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
