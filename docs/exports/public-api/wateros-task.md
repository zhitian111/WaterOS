# wateros-task — 聚合层公共 API

## 用途

列出根 crate `wateros` 通过 `task` 依赖最终使用的对外接口。契约类型来自 `wateros-task-api-v0`；调度与 TCB 细节见子 crate rustdoc。

事实来源：`os/components/wateros-task/src/lib.rs`；根 `os/Cargo.toml` 中 `task = { package = "wateros-task", ... }`。

## 模块树

```text
task::
  # 再导出 api_v0 类型（TaskId, UserTask, ProcessDescriptor, ...）
  sched::*              # Linux sched_* 原语
  wait_queue::WaitQueue # 同步对象侧等待队列封装
  runtime（私有）        # trap/C ABI 入口，经 unsafe 薄封装对外
```

## 初始化

| 项 | 说明 |
|----|------|
| `init()` | 进程 registry 自检 + 调度器 `init`；任何 spawn 前调用 |
| `run_first_task()` | `-> !` 切入第一批就绪任务 |

## 任务创建与运行

| 项 | 说明 |
|----|------|
| `spawn_kernel_task(entry, arg)` | 创建内核任务并入队 |
| `spawn_user_task` / `spawn_user_task_spec` | 按 `UserTask` 规格创建用户任务 |
| `user_task_from_loaded_elf` / `spawn_user_task_from_loaded_elf` | 从 `mm::LoadedElf` 构造 |
| `yield_now` | 主动让出 CPU |
| `schedule_tick` | 时钟 tick 调度入口 |
| `current_task_id` / `current_task_snapshot` / `task_snapshot` | 任务查询 |
| `current_tick` | 调度器逻辑 tick |

## 阻塞、等待与唤醒

| 项 | 说明 |
|----|------|
| `block_current(reason)` | 按 `TaskBlockReason` 阻塞 |
| `wait_on` / `wait_on_while` / `wait_on_for_ticks` / `wait_on_while_for_ticks` | 通用等待 |
| `task_exit_wait_handle` / `wait_for_task_exit*` | 等待任务退出 |
| `sleep_for_ticks` | 定时睡眠 |
| `wake_task` / `interrupt_task` | 唤醒 / 信号中断等待 |
| `WaitQueue` | `new` / `wait_current*` / `wake_one` / `wake_all` / `requeue_to` |

## fork / clone / exec / 退出

| 项 | 说明 |
|----|------|
| `fork_current` / `abort_fork_child` | fork 子进程 |
| `clone_current_thread` / `abort_clone_thread` | 同进程线程 |
| `execve_current` / `terminate_other_threads_for_exec` | execve |
| `exit_current` / `exit_group_current` / `kill_task` | 退出与杀任务 |
| `reap_exited_task` / `reap_one_exited_task` / `reap_one_exited_child` | 回收退出信息 |
| `reap_exited_process` / `reap_all_exited_processes` / `purge_all_user_processes` | 进程级回收 |

## Trap 与地址空间（trap handler 用）

| 项 | 说明 |
|----|------|
| `begin_current_trap_frame_access(frame)` | `unsafe` 进入 trap 时交换权威帧 |
| `restore_current_trap_frame(frame)` | `unsafe` 返回前写回帧 |
| `current_task_user_aspace_ptr` | 当前用户页表对象指针 |
| `current_task_user_address_space_token` | satp/PGDL token |
| `current_task_trap_return_address_space_token` | trap 返回用 token |

## 进程 registry（syscall / signal 用）

| 项 | 说明 |
|----|------|
| `process_snapshot` / `current_process_snapshot` | 进程语义快照 |
| `process_task_snapshot` / `current_process_task_snapshot` / `current_thread_id` | 线程归属 |
| `all_process_pids` / `task_ids_for_process` / `leader_task_for_process` | 枚举与反查 |
| `process_resource_limit` / `set_process_resource_limit` | rlimit |
| `process_nice` / `set_process_nice` / `process_pgid` / `set_process_pgid` | nice 与进程组 |
| `find_exited_child_process*` / `find_stopped_child_process*` / `find_continued_child_process*` | wait 路径 |
| `stop_process_tasks` / `continue_process_tasks` | SIGSTOP/SIGCONT |
| `mark_process_stopped` / `mark_process_continued` / `consume_*_wait` | stopped/continued 状态机 |
| `set_task_clear_child_tid` / `task_clear_child_tid` | futex/clear_tid |
| `has_child` / `has_child_process` / `has_child_process_in_pgid` | 子进程存在性 |
| `create_session_for_process` | setsid |
| `process_dumpable` / `process_child_subreaper` 及 setter | prctl 子集 |
| `process_model_self_test` | bring-up 自检 |

## `task::sched`（Linux `sched_*`）

| 项 | 说明 |
|----|------|
| `resolve_sched_pid` | pid/tid → 内部 `TaskId` |
| `get_scheduler` / `set_scheduler` / `get_param` / `set_param` | 策略与参数 |
| `fill_cpu_affinity_mask` / `cpu_affinity_ret_bytes` / `validate_cpu_affinity_buf_len` / `set_affinity` | CPU 亲和性（单核） |

## 主要再导出类型（`api_v0`）

`TaskId`, `TaskState`, `TaskSnapshot`, `UserTask`, `ExitedTask`, `TaskWaitHandle`, `TaskWaitResult`, `ProcessId`, `ThreadId`, `ProcessDescriptor`, `CloneFlags`, `SchedPolicy`, `SchedParam`, `SchedError`, …

## 初始化契约（根 crate 责任）

1. `task::init()` — 在 `spawn_*` 与 `run_first_task` 之前
2. MM 初始化与用户 ELF 装载 — 见 `wateros-mm` 文档
3. `trap_handler::init` 注册 arch trap 后，经 `task` runtime 符号进入用户态

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
