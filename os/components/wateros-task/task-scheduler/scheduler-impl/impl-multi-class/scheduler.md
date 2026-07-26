
* `sched_param.sched_priority` 校验是错的。
  现在 [sched.rs (line 116)](/Users/x/code/WaterOS/os/components/wateros-task/src/sched.rs:116) 对所有 policy 都要求 `1..=99`，但 Linux 规定：

  * `SCHED_OTHER` / `SCHED_BATCH` / `SCHED_IDLE`：必须为 `0`
  * `SCHED_FIFO` / `SCHED_RR`：`1..=99`

  这会让常规的 `sched_setscheduler(..., SCHED_OTHER, { .sched_priority = 0 })` 错误返回 `EINVAL`。这很可能也是 cyclictest 部分失败的直接原因。
* FIFO/RR 目前是“部分实现”，且权限缺失。
  队列、优先级和 RR tick 已有，但用户态目前可直接把自己设为 FIFO/RR 99；应先加权限检查，否则普通进程可饿死系统。FIFO 同优先级不应因普通 reschedule/yield 以外的路径被轮转，现有选队逻辑也还需单测确认。
* CFS 缺少调度粒度，容易每 tick 轮换。
  当前当前任务只要 `vruntime > ready_min_vruntime` 就切换；多个同权重任务时很容易一 tick 一切。应引入最小运行粒度/目标延迟，至少避免过于频繁的 context switch。这和你之前担心的 cyclictest 性能问题直接相关。
* `min_vruntime` 推进不完整。
  现在主要在从 ready tree `pick()` 时更新；当前任务连续运行期间，CPU 的 CFS 基线并未随之推进。此时新唤醒任务会按偏旧基线归一化，可能获得过多补偿。应让“当前任务 vruntime + ready tree 最小值”共同单调推进该 CPU 的基线。
* `nice` 修改没有完整生效。
  [`set_nice` (line 277)](/Users/x/code/WaterOS/os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/tasks.rs:277) 只更新 TCB；正在运行任务的 CPU cache 仍保留旧 `current_nice`，直到下次切换才生效，也不会触发应有的重新评估。应像策略切换一样同步当前统计、更新运行 CPU cache，并按需请求重调度。
* SMP 负载估计漏掉正在运行的任务。
  `pick_cpu_for_new_task()` 用 ready queue 长度选核；一个 CPU 即使正在跑长期 CPU-bound 任务，其 load 仍可能是 0。放置策略应至少计入非 idle 的 current task；后续再做 idle pull / 定期负载均衡。
* CFS 唤醒抢占尚未实现。
  当前 `SCHED_OTHER` 新任务入队后不会因较小 vruntime 立即抢占，只会等下一次 tick 判断。这个行为可作为 `SCHED_BATCH` 的基础，但 `SCHED_OTHER` 最终应增加带阈值的 wakeup preemption。
