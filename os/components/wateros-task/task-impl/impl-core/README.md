# Task Core 实现手册

[Task 总览](../../README.md) · [Task API](../../task-api/api-v0/README.md)

`impl-core` 落实两个核心所有者：`TaskControlBlock` 持有单条执行流的栈、上下文、trap frame 和调度属性；`ProcessRegistry` 持有 PID/TID、线程组、父子关系和 wait 可见状态。调度器拥有 TCB registry，进程 registry 单独全局保存 PCB；跨两者的事务由上层 `wateros-task` 编排。

## TCB 数据与生命周期

`TaskControlBlock` 的 `TaskInner` 分为 Idle、Kernel、User。Idle 也必须持有 `KernelResources`，否则初始上下文中的 SP/bootstrap 指针会在构造后悬空。User 持有独立内核栈、架构 trap frame 和 `UserTask` 映像描述。

关键不变量：

- `task_cx` 是内核上下文切换现场，`trap_frame` 是返回用户态现场，两者不能相互替代。
- fork 创建独立 TCB、内核栈、trap frame 和地址空间句柄，子返回值为 0；clone thread 创建独立执行上下文，但进程资源按 flags 共享。
- `ready_cpu_id` 仅在已发布 Ready 时存在；`running_cpu_id` 是 Running 状态 CPU 的唯一真相；`last_cpu_id` 可跨阻塞保留。
- affinity 是允许集合，实际目标还必须与 configured/online CPU 相交。
- `execve_from` 替换用户映像/trap/token，但当前执行流和调度身份继续存在。

## PCB 与索引

`ProcessControlBlock` 保存 pid、parent、leader、地址空间引用、PGID/SID、线程表、rlimit、umask、caps、PDEATHSIG、subreaper、stop/continue wait 标志等。`ProcessRegistry` 同时维护：

```text
processes: PID -> PCB
pid_for_task: TaskId -> PID
task_for_thread: TID -> TaskId
```

任何创建、退出或 reap 都必须三表一致。非 leader 线程退出先进入 `exited_member_task_ids`，便于精确回收 TCB；整个进程退出后 PCB 作为 zombie 留给父进程 wait。`RetiredProcess::cleanup` 在 registry 锁外销毁地址空间，避免 MM 写回/释放链路重入 task 锁。

`exec_in_progress` 阻止 exec 清理线程组时注册新 member。进程 I/O 计数使用 PCB 内 `Arc<ProcessIoCounters>`，fork 新建、线程共享；每 CPU cache 只减少热路径 registry 查找，不能成为计数所有者。

## 创建与回滚链

```text
上层 create/fork/clone
  -> scheduler registry 创建未入队 TCB
  -> ProcessRegistry 创建 PCB 或加入线程
  -> 初始化 fd/cred/signal/futex 等外部侧表
  -> 成功：activate/start，发布到 ready queue
  -> 失败：撤销外部侧表 + abort PCB 记录 + discard_unstarted_task
```

未入队 TCB 是关键事务边界。新增侧表继承时必须放在发布之前，并提供反向清理；任务一旦可运行，失败回滚就会与用户态并发。

## 锁规则

全局 process registry 经 `MultiprocessorSafeCell` 访问，先用 RAII 保存并关闭本地中断，退出时恢复原状态。锁内只做 map/index/state 变更；地址空间销毁、可能阻塞的写回、IPI 和复杂外部 hook 放到锁外。不要在 `with_process_registry` 闭包内再次调用获取同一 registry 的门面函数。

## 增加 PCB 字段的检查表

1. 决定 init 默认值、fork 继承、clone 共享、exec 保留/清零、exit/drop 行为。
2. 如需用户查询，加入最小快照字段或专用 getter；不要把可变 PCB 引用泄露出去。
3. 更新 create、fork、snapshot 和测试构造中的所有初始化点。
4. 若字段引用外部资源，设计 `RetiredProcess` 锁外清理路径。
5. 测试 PID/TID 复用、并发退出/wait、leader 先退出、exec 多线程屏障和子收养。

