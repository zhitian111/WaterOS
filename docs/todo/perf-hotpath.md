# 性能优化：内核热调用路径（syscall 分发 / trap / 上下文切换 / 调度）

## 用途

汇总内核「每次都走、与业务无关的固定开销」路径上的性能瓶颈与改进方案，覆盖：用户态 trap 往返、syscall 分发、用户内存拷贝、TLB/ASID 刷新、上下文切换、调度器就绪/等待队列、定时器 tick。这些路径在 LTP / busybox 压测中被高频触发（日志中 `[syscall]`、`[trap]`、`[exit]` 标签密集），是吞吐与延迟的基础税。

## 事实来源

- 代码静态链路分析（riscv64 + loongarch64 双架构）。
- 日志佐证：`os/ltp_log/rv_ltp_glibc_local_all.log` 等中 `[syscall]`/`[trap]`/`[exit]` 高频出现；`clone`/`mount`/`ioctl` 警告 1500+ 条说明 syscall 分发与 trap 返回路径密集。
- 关联子链路分析见 [hotpath-subagent](48f8b89e-5c0e-4728-9bd7-2c4b04f26840)。
- 交叉参考：`docs/audits/lock-inventory.md`（调度器 / wait_queues 复杂度）、`docs/audits/resource-inventory.md`（WaitQueueId 无上限）。

## 覆盖范围

`os/components/wateros-syscall/syscall-api`、`syscall-impl/impl-kernel`、`os/src/trap_handler.rs`、`wateros-platform/platform-arch/arch-impl/impl-riscv64`、`impl-loongarch64`、`wateros-task/task-scheduler`、`task-impl/impl-core`、`wateros-mm` 的用户拷贝接口。

---

## 优化点清单（按预期收益从高到低）

### H-1. RISC-V 用户态 trap 往返存在 TrapContext 多重拷贝 + 多次全局 TLB flush 【高】

- **位置**：
  - `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm:86-102,211-229,275-296`
  - `os/src/trap_handler.rs:116-128,314-318`
  - `os/components/wateros-task/task-impl/impl-core/src/tcb.rs:438-456`
- **当前实现/复杂度**：用户 trap → trampoline 37×8B 循环拷到内核栈 → Rust 整帧（296B）写入 TCB → 返回前写回栈 → 再拷回 trampoline；每次至少 2 次 `sfence.vma` 全局 TLB 冲刷，`trap_handler` 可能第三次 `activate_address_space_token_and_flush`。属与业务无关的固定开销。
- **问题**：每次 syscall / 页错 / 定时器中断都付出 296B×4 拷贝 + 2~3 次全局 flush。
- **改进方案**：单缓冲权威帧（直接在 TCB/trampoline 处理，去掉栈↔TCB↔trampoline 往返）；内核 satp 切换后已 flush 则用 token 比较跳过重复 flush；同 ASID 用 `sfence.vma` 按 ASID 局部刷新（见 H-4）。
- **预期收益**：高，所有用户态 trap 的基础税。
- **架构差异**：RV 显著重于 LA；LA 在 `trap.S:85-147` 直接在内核栈建帧，无 trampoline 二次拷贝，回用户仅一次 `invtlb`（`trap.S:161-165`）。
- **风险/依赖**：与 trampoline 用户页映射、`sscratch` 约定、GDB 断点强耦合；跨 arch + task + trap_handler。

### H-2. 用户内存拷贝每页重复软件 walk，路径串逐字节拷贝 【高】

- **位置**：
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/user_access.rs:81-167`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:604-621,950-977`
  - `os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs:85-110`
- **当前实现/复杂度**：`copy_from_user_in_aspace` 每 chunk 先 `translate_addr`（3 级 walk）再 `leaf_page_perm`（再 walk），每页边界约 6 次 PTE 访问；`copy_to_user` 叠加 COW 检查可达 3× walk/页。`copy_user_path_cstr` 循环内逐字节 `copy_from_user`，路径长 L 即 O(L) 次完整 walk，而 openat/stat 等路径 syscall 极热。
- **问题**：read/write/ioctl 与所有路径类 syscall 的主导成本；LTP/busybox 大量短路径字符串放大效应。
- **改进方案**：将 `translate + perm` 合并为单次 walk 并在拷贝循环内缓存 `(vpn → pa, perm)`；路径串改 `strnlen` 式批量读（一次读至页末或 NUL）；热路径可引入软件 VA→PA 小缓存（类 Linux `__copy_user`）。
- **预期收益**：高，IO 与路径 syscall 主瓶颈。
- **架构差异**：算法相同（LA `impl-loongarch64/src/user_access.rs:92-128` 同构）。
- **风险/依赖**：合并 walk 需与 `handle_lazy_page_fault` 的 COW/懒分配语义一致。

### H-3. Syscall 分发存在双重 decode（if-else 链 + 巨型 match）【高】

- **位置**：
  - `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs:162-444,1757-1899`
  - `os/src/trap_handler.rs:341-353`（`restartable_syscall` 再次 decode）
  - `os/components/wateros-syscall/syscall-impl/impl-kernel/src/lib.rs:687-717`（unknown 又一次 if 链）
- **当前实现/复杂度**：`SyscallKind::decode` 约 140 个 `if syscall_nr == T::XXX` 线性比较，最坏 O(n)（n≈140）；随后 `dispatch_syscall_from_trap` 对 `SyscallKind` 再 match 约 140 分支；EINTR 重启路径第三次 decode。
- **问题**：每次 syscall 固定支付 2~3 次分发；编译器难完全优化为跳表；I-cache 压力大。
- **改进方案**：构建期生成 `[max_nr] → handler` 稠密/稀疏跳表（或对号表索引 match）；trap 层直接 `handlers[nr](args)`，去掉 `SyscallKind` 中间层；重启判定用位图 O(1)。
- **预期收益**：高，所有 syscall 入口。
- **架构差异**：无（RV/LA 共用 dispatch）。
- **风险/依赖**：需与 `ActiveSyscallNumberTable`、旁路号（statx/fstatat）代码生成策略统一。

### H-4. 已有 ASID 字段却仍全局 sfence / invtlb 【中高】

- **位置**：
  - `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/src/paging.rs:25-29`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:158-192`
  - `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/src/paging.rs:52-55,161-165`
- **当前实现/复杂度**：RV `make_satp` 分配递增 ASID（`:181-192`），但 `activate_address_space_token_and_flush` 恒 `sfence.vma x0,x0` 全局冲刷；LA 无 ASID，每次 `write_pgdl` + `invtlb 0,$zero,$zero` 全局无效化。
- **问题**：用户↔内核 trap、任务首次运行、exec 切页表都全 TLB 失效，多任务下 IPC/syscall 延迟放大；ASID 完全失效。
- **改进方案**：RV 同 ASID 仅 `sfence.vma` 按 ASID/VA，跨 aspace 才全局；LA 引入 ASID 域或减少 PGDL 切换；ASID 耗尽时按 generation 批量回收。需与 H-7（ASID generation/shootdown）配套。
- **预期收益**：中高，trap 返回与 exec/fork 密集场景。
- **架构差异**：RV 有 ASID 字段未用；LA 始终全局 invtlb，更重。
- **风险/依赖**：硬件语义、QEMU 行为、与 COW/fork 页表更新顺序。

### H-5. trap_handler 用户 trap 入口重复激活内核页表 【中】

- **位置**：`os/src/trap_handler.rs:123-128`、`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm:88-91`
- **当前实现/复杂度**：RV 汇编已切内核 satp + flush；Rust 再读 `active_address_space_token()` 与 `kernel_satp()` 比较，不等则第二次 flush。
- **问题**：正常路径恒等比较仍有一次 CSR 读 + 分支；异常时双倍 flush。
- **改进方案**：汇编切页后置「已在 kernel satp」标志或信任帧内 token；合并为 arch 层 `ensure_kernel_aspace()` 单次调用。
- **预期收益**：中（RV 专属）。
- **架构差异**：主要 RV；LA trap 入口不切 PGDL。
- **风险/依赖**：内核态嵌套 fault 路径需仍正确。

### H-6. 每次 syscall 返回都查 pending signal（抢全局 registry 锁）【中】

- **位置**：`os/src/trap_handler.rs:283-285,327-338`、`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs:249-256`
- **当前实现/复杂度**：凡 `returns_to_user()` 均 `deliver_pending_signal` → `ensure_current_signal_state` + `SignalRegistry` 锁查表；无 pending 也 early-return，但锁与查表是固定税。
- **问题**：高频 getpid/read/write 无信号场景也付 registry 成本。
- **改进方案**：TCB 缓存 `pending/deliverable_bits`（类 Linux `TIF_SIGPENDING`），为 0 则无锁跳过；仅 `raise_signal` 置位、deliver 后清除。与 IPC 文档 I-8 协同。
- **预期收益**：中，syscall 密集 workload 的返回路径。
- **架构差异**：无。
- **风险/依赖**：mask/sigsuspend/ppoll 临时 mask 与 cache 一致性。

### H-7. wait queue 唤醒 / detach 多队列线性扫描 + retain 【中】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:112-141,306-378,625-628`
- **当前实现/复杂度**：`finish_blocked_task` 依次扫 `blocked_queue`、`sleep_queue`、全部 `wait_queues[]`、exit/child_exit 队列；`take_task_id_by_id` 用 `VecDeque::retain` O(n)；`detach_task_from_run_queues` 对所有 wait 槽 O(W×n)。
- **问题**：futex/poll/connect 唤醒、线程退出、kill 随 wait 槽与任务数线性变慢；并与 IPC I-3（exit 不回收空 futex 队列）叠加使 W 无界增长。
- **改进方案**：阻塞时在 TCB 记录 `current_wait_handle (queue_id, index)` 或用 intrusive 链表节点，唤醒 O(1) 定位；或 `HashMap<TaskId, WaitLocation>` 替代全表扫描。
- **预期收益**：中，多线程 + 多 futex/poll 场景。
- **架构差异**：无（RR/multi-class 共用 `WaitQueues`）。
- **风险/依赖**：与 `allocate_wait_queue` id 复用、超时队列一致性。

### H-8. multi-class RT 队列 remove / highest_priority 线性扫描 99 桶 【中】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/rt_fifo_queue.rs:52-100`、`scheduler.rs:54-61`
- **当前实现/复杂度**：`RtFifoRunQueue::remove` 对 99 个优先级桶逐个 `take_task_id_by_id`，最坏 O(99×n)；`highest_runnable_priority` 每 tick 可能扫全部桶。
- **问题**：RT 任务 block/unblock/kill 频繁时调度器 CPU 占用高。
- **改进方案**：RT 桶采用 per-task version + lazy pop（与 `OtherReadyQueue` 同构）；或 `TaskId → (bucket, index)` 索引 O(1) remove；非空最高优先级用位图/堆维护。
- **预期收益**：中（启用 multi-class 且 RT 线程多时）。
- **架构差异**：无（仅 multi-class 配置）。
- **风险/依赖**：`SchedPolicyChangeAction`、RR 时间片状态机。

### H-9. 大缓冲 read/write 每次堆分配最高 4 MiB 并清零 【中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/fallible_buf.rs:9-33`、`sys/read.rs:45-64`
- **当前实现/复杂度**：`len > 256` 时 `try_kbuf(len, SYSCALL_IO_MAX)` 分配最多 4MiB `Vec` 并零初始化，每次大 IO 一次堆分配+清零+释放；小 read 用 256B 栈缓冲较优。
- **问题**：吞吐测试/大块 IO 触发 allocator 锁与碎片。
- **改进方案**：固定 64KiB 栈/静态池分块循环 `read→copy_to_user`；或 per-task syscall 缓冲 slab 复用。
- **预期收益**：中，大块 IO benchmark。
- **架构差异**：无。
- **风险/依赖**：pipe/socket 原子读语义、EINTR 部分写进度。

### H-10. decode 对高频 syscall 无 fast path（H-3 子集 / quick win）【中】

- **位置**：`os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs:163-168,311-316`
- **当前实现/复杂度**：decode 按枚举顺序比较，最热的 read(63)/write(64)/futex/clock_gettime/getpid 排在链中后段，平均比较约 70+ 次。
- **问题**：在 H-3 跳表落地前可低成本改善典型 workload。
- **改进方案**：将 Top-16 热号提到 decode 前端或单独分支表；或按频率排序比较链（PGO）。
- **预期收益**：中（H-3 的子集，二选一以免重复维护）。
- **架构差异**：无。

### H-11. 定时器 tick 在时间片未耗尽时仍 promote sleep/timeout 队列 【中低】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-impl/impl-round-robin/src/scheduler.rs:141-154`、`os/src/trap_handler.rs:251-261`、`wait_queues.rs:182-195`
- **当前实现/复杂度**：每次 `SupervisiorTimer` → `schedule_tick` → 即使 `current_task_ticks < MAX_TICKS` 仍 `promote_sleeping_tasks` + `promote_wait_timeouts`；sleep 插入本身 O(n)。
- **问题**：固定频率 tick，无调度事件时也扫描 sleep/timeout 队列。
- **改进方案**：空队列 flag 时跳过 promote；用 timer wheel / 堆按 next_wake 武装单次定时器（与 H-12 统一）。
- **预期收益**：中低，idle/light load 降中断处理量。
- **架构差异**：无；LA `TIMER_SLICE_TICKS` 更大，相对 tick 密度更低（`trap.rs:58`）。
- **风险/依赖**：futex 超时精度、nanosleep 语义。

### H-12. sleep / wait_timeout 有序插入 O(n) 【低中】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:182-195,221-227`
- **当前实现/复杂度**：`sleep_queue` 与 `wait_timeouts` 用 `iter().position` 找插入点，O(n) 插入。
- **问题**：大量不同 wake_tick 的 sleep 并发时插入慢。
- **改进方案**：binary heap / timer wheel 按 wake_tick 组织；与 H-11 统一为单一 timer 子系统。
- **预期收益**：低中，大量定时 sleep 并发。
- **架构差异**：无。

### H-13. ready 队列 stale entry 排空：pick_next 均摊 O(stale) 【低中】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-impl/impl-round-robin/src/queues.rs:70-84`、`impl-multi-class/src/queues.rs:78-92`
- **当前实现/复杂度**：version bump 使 enqueue/detach O(1)（设计良好），但 `pick_next` 需 pop 并丢弃 stale entry，最坏 O(队列长度)；高频 yield/block 时队列含大量幽灵项。
- **问题**：线程池式 churn 下调度 pick 退化。
- **改进方案**：stale 比例超阈值时 compact，或 intrusive 链表真正删除（detach 时 O(1) 移除，需索引）。
- **预期收益**：低中，高 churn 多线程。
- **架构差异**：无。

### H-14. 上下文切换 `__switch` 不保存 FPU/向量（多任务 FP 正确性 + lazy FPU 机会）【中低 / FP 场景为高】

- **位置**：`os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/switch.S:13-29`、`impl-riscv64/src/trap.rs:49-77,186-188`、`impl-loongarch64/src/trap.rs:59-63`
- **当前实现/复杂度**：`__switch` 仅保存 ra+sp+callee-saved，O(1)，不切页表（设计正确）；FPU 仅 signal 路径 `save_fp_state`，RV 返用户恒置 `FS=Dirty`。
- **问题**：多任务 + hard-float libc 下 FPU 寄存器可能串任务（正确性）；无法 lazy FPU（性能）。
- **改进方案**：per-task `fp_used` + lazy FPU（首次 FP 指令 trap 再保存/恢复 32×64bit + fcsr）；RV 可缩小 trap 帧仅在有 FP 时扩展。
- **预期收益**：中低（多任务 FP workload 时为高，且偏正确性）。
- **架构差异**：LA 注释已承认 bring-up 串行假设；RV 同理。
- **风险/依赖**：信号帧、rt_sigreturn、LTP math 测例。

### H-15. page fault 处理链路过长（trap → 全局 MM 分发 → 多次帧锁）【中】

- **位置**：`os/src/trap_handler.rs:187-216`、`os/components/wateros-mm/src/lib.rs:99-128`
- **当前实现/复杂度**：用户页错 store fault 先尝试 `handle_cow_fault` 再 `handle_user_page_fault`（两次 trap 尝试）；每次成功 fault 全局 flush + 多次 `with_frame_allocator`。
- **问题**：稀疏堆栈/mmap 首次 touch fault 频率高，与 syscall 软件 walk 重复劳动。
- **改进方案**：合并 fault 解码一次分发；fault 成功用 VA 定向 flush；COW 批量处理同 VMA 多页。与内存文档 M-9、M-20 协同。
- **预期收益**：中，fork/COW/大块 mmap 场景。
- **架构差异**：LA 硬件 TLB refill 较快，但 Rust fault handler 开销仍存在。

### H-16. 热路径 trace 日志与失败路径 probe 重复 walk 【低（默认关闭）/ 调试时中】

- **位置**：`os/src/trap_handler.rs:139-149,290-296`、`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/read.rs:75-79`、`user_copy.rs:62-68`
- **当前实现/复杂度**：syscall 路径 `trace!` 打印 6 参数 + PC/SP；RV copy 失败 `debug_probe_user_virt` 额外 walk。
- **问题**：DEBUG/TRACE 级别开启时显著拖慢；probe 在失败路径重复 walk。
- **改进方案**：热路径 trace 用 `cfg!(debug_assertions)` 或 `feature = "syscall-trace"` 门控；probe 仅在显式 debug feature 启用。
- **预期收益**：低（默认无影响），调试配置下中。
- **架构差异**：RV 失败路径 probe 更重（LA 无 probe）。

---

## 落地优先级建议

| 优先级 | 条目 | 一句话 |
|--------|------|--------|
| P0 | H-1, H-2, H-3 | 降 trap 税、合并页表 walk、syscall 跳表 |
| P1 | H-4, H-5, H-6 | ASID 惰性 TLB、去重 flush、signal fast path |
| P2 | H-7, H-8, H-9, H-10 | 等待队列索引、RT 队列、IO 分块、热号 fast path |
| P3 | H-11~H-16 | tick 合并、timer 结构、FPU lazy、队列 compact、日志门控 |

## 后续维护入口

- 改动 syscall 分发：同步 `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs` 与 `docs/exports/features/wateros-syscall.md`。
- 改动调度/等待队列：同步 `wateros-task` 各 `Cargo.toml` 与 `os/src/main.rs` 中断/定时器接线，参考 `docs/audits/locks/scheduler` 相关分组。
- 改动 trap/ASID/TLB：同步两架构 `arch-impl` 与本文档、`perf-memory.md`（M-1/M-2）。
