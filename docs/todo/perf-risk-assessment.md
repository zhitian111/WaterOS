# 性能优化：风险收益评估与安全实施指南

## 用途

为 `docs/todo` 下全部约 95 条性能改进点提供统一的**风险 × 收益评估**与**安全实施流程**，回答「这些优化能否在不引入新 bug 的前提下完成」。本文件是风险维度的单一事实来源；各子系统文档（`perf-hotpath.md` 等）末尾的「风险与验证速查」表是本文件对应章节的内联子集。

> 本目录只产出分析与方案，不改代码。风险评估基于代码链路分析与项目并发模型，**不替代实测**；实施前仍须按本文件「安全实施流程」逐项验证。

## 评估口径

- **收益**：高 / 中 / 低（沿用各条目文档结论，指吞吐或延迟改善幅度 × 影响面）。
- **风险**：低 / 中 / 中高 / 高，指**引入新 bug 的概率 × 排查难度**。
  - 低 = 行为保持型或纯局部替换，回归即可暴露问题。
  - 中 = 触碰数据结构不变量/缓存失效/锁序，需配断言与定向测例。
  - 中高 = 触碰并发竞态、页表生命周期、回收语义，错了可能偶发。
  - 高 = 触碰 TLB 一致性、底层 trap、上下文状态，错误**静默且非确定性**，最难定位。
- **Flag**：是否建议先用 `feature` 开关挂新旧两条路径、默认旧路径、对拍切换。
  - 是 = 强烈建议；建议 = 视实现复杂度；否 = 行为保持，可直接替换。
- **验证**：除每项一轮 `make rv_check`+`make la_check`+全量 LTP/busybox 回归外的关键定向手段。

## 这个项目特有的风险来源（实施前必读）

1. **单核 + 定时器抢占 + `UniprocessorSafeCell`（基于 `RefCell` 的伪锁）**。
   `os/components/wateros-base/src/sync/uniprocessor.rs` 提供的伪锁靠运行时借用检查。凡是「释锁后再做」「缩短/移动关中断窗口」的优化（L-1/L-2/L-3/L-11/L-18、I-2 等），一旦把工作错误地移出关中断守卫，定时器抢占下重入同一 cell 就会触发 **`RefCell` 双重借用 panic**（审计 R-PT-11）。这类改动在本项目比真 SMP 更易翻车，必须保持「持借期间不再进入会回借同一 registry 的路径」。

2. **TLB / ASID 是静默错误重灾区**。
   M-1/M-2/H-4/L-16 把全局 flush 改成选择性 flush，一旦漏刷一次或复用 ASID 未做 generation/shootdown，表现是**用户进程偶发读到旧映射 → 数据错乱 / UAF**。LTP 可能照常通过，但特定时序下崩溃，事后极难复现。必须增量灰度 + Flag + 断言。

3. **页表生命周期改动牵一发动全身**。
   M-3（fork 页表 COW）、M-5（munmap 回收中间页表帧）、M-4（destroy 批量化）相互耦合，且与「内核恒等映射共享的中间节点」「MAP_SHARED refcount（M-18）」强相关。任一回收/共享语义不闭环就可能误释放或泄漏。

4. **有些「优化」其实是修现存 bug**。
   M-18、F-2、F-6、F-7、I-10、I-12 等本身就是隐患（丢脏、不回收、永久睡眠）。不做才是带 bug；做了是净减风险，但实现仍可能引入回归，故仍按对应风险等级验证。

## 风险分层总览

### 第 1 层：低风险（行为保持，建议最先做，建立回归基线）

`M-8`、`M-17`、`H-3`、`H-10`、`H-16`、`I-13`、`I-15`、`F-14`、`F-20`、`F-21`、`L-17`、`L-14`（纯加清理）。

这些**可在不引入新 bug 的前提下完成**，前提是每改一项跑一轮 RV+LA 回归。

### 第 2 层：中 / 中高风险（接口不变但触碰不变量或锁序，需断言 + 定向测例，部分建议 Flag）

热路径 `H-2,H-5,H-6,H-7,H-8,H-9,H-11,H-12,H-13,H-15`；内存 `M-6,M-7,M-9,M-10,M-11,M-12,M-14,M-15,M-16,M-19,M-20`；FS `F-1~F-8,F-10~F-13,F-15~F-18`；IPC `I-1~I-12,I-14,I-16,I-17`；锁回收 `L-1~L-13,L-15,L-18,L-19`。

### 第 3 层：高风险（静默/非确定性错误，必须 Flag + 灰度 + 增量验证）

`H-1`（trap 重构）、`H-14`（lazy FPU）、`M-1`、`M-2`、`M-3`、`M-5`。

### 修 bug 类（净收益为正，仍按等级验证）

`M-18`、`F-2`、`F-6`、`F-7`、`I-10`、`I-12`。

## 安全实施流程（建议固化为每项的 checklist）

1. **先排低风险项**，确认 RV+LA 的 LTP/busybox 回归基线稳定、可复现。
2. **高风险项一律 Flag 化**：新旧路径可切，默认旧路径，新旧对拍。
3. **TLB/ASID 增量化**：先仅对「同地址空间 trap 往返」启用选择性 flush，跨 aspace 仍全局；配 ASID generation/shootdown；用 `make rv_pc_watch`、压力测例观察后再放开。
4. **加 debug 断言**：页缓存 `index/free/lru` 三表一致、fd/unix/Registry 索引与主表一致、ASID generation 匹配——让 bug 在测试期 panic 而非上线静默。
5. **锁序改动对照审计**：L-1/L-2/L-3/L-11/L-18、I-2 改前对照 `docs/audits/locks/*`，确认未把工作错误移出关中断守卫，避免 R-PT-11 类 RefCell panic。
6. **最小可回滚步骤**：M-1/M-3/H-1 这类底层路径拆成小步逐步合入，每步独立可回退。
7. **每项验证三连**：`cd os && make rv_check && make la_check` → 定向测例 → 全量 LTP/busybox 回归（RV+LA 双跑）。

---

## 全量条目矩阵

> 列：编号 | 收益 | 风险 | 主要风险类型 | Flag | 关键验证（在三连基础上）

### 热路径（perf-hotpath.md）

| 编号 | 收益 | 风险 | 风险类型 | Flag | 关键验证 |
|------|------|------|----------|------|----------|
| H-1 RV trap 多重拷贝+flush 单缓冲化 | 高 | 高 | 底层 trap / trampoline·sscratch 耦合 | 是 | 启动 + 全量 LTP + `rv_pc_watch` + GDB 断点 |
| H-2 用户拷贝合并 walk + 路径批量读 | 高 | 中高 | 须与 COW/lazy fault 一致 | 建议 | 路径类 syscall + mm fault 测例 |
| H-3 syscall 分发跳表 | 高 | 低 | 行为保持（纯分发重排） | 否 | 全量 LTP |
| H-4 ASID 选择性 TLB flush | 中高 | 高 | TLB 一致性 | 是 | 与 M-1/M-2 同步增量灰度 |
| H-5 trap 入口去重复激活内核页表 | 中 | 中 | 嵌套 fault 正确性 | 建议 | 内核态异常/嵌套 fault 路径 |
| H-6 syscall 返回 pending signal TCB 快表 | 中 | 中 | signal mask 一致性 | 建议 | signal LTP（含 ppoll/sigsuspend） |
| H-7 wait queue O(1) 索引 | 中 | 中 | TCB 反向指针/迁移一致 | 否(+断言) | futex/poll LTP |
| H-8 RT 队列 version/索引 | 中 | 中 | multi-class 状态机 | 否 | sched 测例（仅 multi-class 配置） |
| H-9 大 IO 分块缓冲 | 中 | 中 | EINTR/原子读语义 | 否 | pipe/socket/read LTP |
| H-10 热号 fast path | 中 | 低 | 行为保持 | 否 | 全量 LTP |
| H-11 tick 跳过空 promote | 中低 | 低中 | 超时精度 | 否 | nanosleep/timer 测例 |
| H-12 sleep 堆 / timer wheel | 低中 | 中 | 超时正确性 | 否 | timer/futex 超时 |
| H-13 ready 队列 compact | 低中 | 低中 | 队列不变量 | 否 | 高 churn 多线程 sched |
| H-14 lazy FPU | 中低 / FP 场景高 | 高 | 上下文状态串污染 | 是 | math LTP + 多线程 FP |
| H-15 page fault 合并分发 | 中 | 中 | COW/lazy 顺序 | 建议 | mm fault 测例 |
| H-16 trace 日志门控 | 低 | 低 | 编译期开关 | 否 | 编译验证 |

### 内存（perf-memory.md）

| 编号 | 收益 | 风险 | 风险类型 | Flag | 关键验证 |
|------|------|------|----------|------|----------|
| M-1 选择性 TLB flush | 高 | 高 | TLB 一致性 | 是 | 增量（先同 aspace）+ 压测 + 断言 |
| M-2 ASID generation + shootdown | 高（前置） | 高 | TLB 一致性 / 复用 ASID | 是 | 同 M-1；ASID 回绕用例 |
| M-3 fork 页表延迟复制（页表 COW） | 高 | 高 | 页表生命周期 / break-COW | 是 | fork 压测 LTP + 帧账本断言 |
| M-4 destroy 批量化 | 高 | 中高 | 回收正确性 | 建议 | exit 压测 + 帧账本断言 |
| M-5 munmap 回收中间页表帧 | 高 | 高 | 误删共享/恒等节点 | 是 | mmap 抖动 + 断言 |
| M-6 brk/anon 零页策略 | 高 | 中 | 安全清零（信息泄漏） | 建议 | 新页内容置零校验 |
| M-7 VMA 区间树空闲管理 | 中高 | 中 | 放置正确性 | 否 | mmap/mremap LTP |
| M-8 去 `recycled.contains` 冗余 | 中 | 低 | 行为保持（位图已覆盖） | 否 | 帧分配测例 |
| M-9 COW 合并锁 + 单 VA flush | 中 | 中高 | refcount + TLB | 建议 | fork 后写 LTP |
| M-10 PTE 缓存 / 大页 | 中 | 中 | 缓存一致 | 建议 | 全量 + 大块拷贝 |
| M-11 map 失败回滚 | 中 | 低中 | 错误路径账本 | 否 | OOM 注入 |
| M-12 帧元数据静态化 | 中低 | 中 | 引导顺序 | 否 | 启动验证 |
| M-13 内核堆 slab/buddy | 中低 | 中 | 分配器正确性 | 是 | 全量 + 堆压测 |
| M-14 lazy VMA 区间树 | 中低 | 中 | VMA 一致 | 否 | 多映射 mmap |
| M-15 文件 mmap 默认 lazy | 中低 | 中 | 用户帧 vs 页缓存双份 | 建议 | mmap 文件读写 |
| M-16 mremap move PTE | 中低 | 中高 | break COW | 建议 | mremap 测例 |
| M-17 表帧清零 `write_bytes` | 低中 | 低 | 行为保持 | 否 | 编译 + fork |
| M-18 MAP_SHARED refcount | 低（性能） | 正确性 P0 / 实现中 | 共享页回收 | 建议 | shared anon 测 + 断言 |
| M-19 brk lazy 统一 | 中 | 中 | 可见内存语义 | 建议 | brk/glibc 分配器测 |
| M-20 fault 链路合并 | 中 | 中 | COW/lazy 顺序 | 建议 | fault 测例 |

### FS / VFS（perf-fs-vfs.md）

| 编号 | 收益 | 风险 | 风险类型 | Flag | 关键验证 |
|------|------|------|----------|------|----------|
| F-1 AuxRo 用 PagedFileHandle | 高 | 中 | 只读卷写回语义 | 建议 | aux 卷大文件读 |
| F-2 unlink 前 flush | 高（正确性） | 中 | 丢脏修复 | 否 | unlink 打开中文件 + 脏页 |
| F-3 dcache / inode 缓存 | 高 | 中高 | 缓存失效（rename/unlink/mount） | 建议 | rename/unlink/mount 失效用例 |
| F-4 页缓存 O(1) LRU | 高 | 中 | 三表不变量 | 否(+断言) | 读写压测 |
| F-5 flush 分段释锁 | 高 | 中 | 锁序 | 否 | 多线程 fsync |
| F-6 mount alias bump flush | 高 | 中 | 丢脏修复 | 否 | mount + 脏页 |
| F-7 sync 覆盖页缓存 | 高 | 中 | 写回覆盖面 | 否 | sync/fsync/fdatasync |
| F-8 install_page 帧复用 | 高 | 中 | 锁外 IO 约定 | 建议 | 读 miss/驱逐压测 |
| F-9 LoongArch 块缓存接线 | 高（LA） | 低中 | cfg 接线 | 是(cfg) | LA ext4 读写 |
| F-10 purge reverse index | 中 | 中 | 索引一致 | 否(+断言) | close/unlink 批量 |
| F-11 mount trie/前缀树 | 中 | 中 | 失效 | 否 | 多挂载点 |
| F-12 ext4 小读缓存移实例 | 中 | 中 | 脏读（多 FS） | 否 | 多 FS 实例 |
| F-13 块缓存 O(1) LRU + write-allocate | 中 | 中 | 缓存一致 | 否 | 写后读 |
| F-14 顺序预取检测 | 中 | 低 | 仅影响预取 | 否 | 随机读 |
| F-15 整文件 read 改 read_range | 中 | 中 | API 调用面 | 否 | 全量（grep 调用面） |
| F-16 get_file_entry 缩临界区 | 中 | 低中 | 锁内嵌套 | 否 | 并发 metadata |
| F-17 rename 迁移缓存键 | 中 | 中 | 缓存失效 | 否 | rename + open fd |
| F-18 detached 共享 Arc | 中 | 低中 | dup 语义 | 否 | unlink 后写 + dup |
| F-19 tmpfs inode freelist | 低 | 低中 | 回收语义 | 否 | 长跑 create/unlink |
| F-20 close 错误日志 | 低 | 低 | 仅可观测性 | 否 | 无 |
| F-21 evict panic 改错误 | 低 | 低 | 可靠性 | 否 | 故障注入 |

### IPC / 同步（perf-ipc-sync.md）

| 编号 | 收益 | 风险 | 风险类型 | Flag | 关键验证 |
|------|------|------|----------|------|----------|
| I-1 poll/select 事件驱动减扫描 | 高 | 中高 | 新子系统 + fd 生命周期 | 否（新功能） | poll/epoll LTP |
| I-2 futex requeue 释锁唤醒 | 高 | 中高 | 并发竞态 + RefCell | 建议 | futex/condvar 压测 |
| I-3 exit 回收空 futex 队列 | 高 | 中 | 反向索引一致 | 否(+断言) | futex + 异常 exit |
| I-4 interrupt O(1) 定位 | 高 | 中 | TCB 反向指针 | 否 | kill/signal 多线程 |
| I-5 pipe wake 策略 | 中 | 中 | 避免回归饿死(P-2) | 建议 | pipe 多读者/多写者 |
| I-6 poll deadline 合并 | 中 | 中 | 超时/借用约束 | 否 | poll/ppoll |
| I-7 signal 进程线程索引 | 中 | 中 | 索引一致 | 否(+断言) | 多线程 signal/kill |
| I-8 TCB pending 快表 | 中 | 中 | mask 一致 | 建议 | signal LTP |
| I-9 real_deadlines 清理 | 中 | 低中 | 索引清理 | 否 | setitimer 长跑 |
| I-10 SHM fork 事务回滚 | 中 / 正确性高 | 中 | fork 回收 | 否 | shm fork 失败注入 |
| I-11 robust 批量 wake | 中 | 中 | 用户链表不可信 | 否 | robust mutex |
| I-12 futex WAIT alternate key | 中 / 正确性高 | 中 | 永久睡眠修复 | 否 | futex private/shared |
| I-13 pipe bulk 拷贝 | 低中 | 低 | 行为保持 | 否 | pipe |
| I-14 sigsuspend 条件读 TCB | 低中 | 中 | 与 I-8 一致性 | 建议 | sigsuspend |
| I-15 free_wait_queues 位图 | 低 | 低 | 行为保持 | 否 | 无 |
| I-16 pipe 临界区抢占 | 低 | 中 | 同步原语 | 是 | SMP 前评估 |
| I-17 epoll 实现 | 高（功能） | 中高 | 新子系统 | 否（新功能） | epoll LTP |

### 锁竞争 / 资源回收（perf-lock-resource.md）

| 编号 | 收益 | 风险 | 风险类型 | Flag | 关键验证 |
|------|------|------|----------|------|----------|
| L-1 reap 锁外 drop aspace | 高 | 中高 | 锁序 + RefCell(R-PT-11) | 建议 | exit/wait 压测 |
| L-2 线程 exit batch drop | 高 | 中 | 锁序 | 建议 | pthread 压测 |
| L-3 fork fd duplicate 释锁 | 高 | 中高 | RefCell + 页缓存嵌套 | 建议 | fork 压测 |
| L-4 unix_sock per-task 索引 | 高 | 中 | 索引一致 | 否(+断言) | unix 多进程 |
| L-5 Registry TaskId 反向索引 | 高 | 中 | 登记窗口原子(PR-01) | 否(+断言) | clone/wait |
| L-6 fd 空闲位图 + open_count | 高 | 中 | 位图同步 | 否(+断言) | open/close churn |
| L-7 exit_group 合并清理 | 高 | 中 | 语义保持 | 建议 | exit_group |
| L-8 waitpid 单次 snapshot | 中 | 中 | TOCTOU | 否 | waitpid 并发 |
| L-9 reap 批量 API | 中 | 中 | zombie 回归 | 建议 | clone/exit |
| L-10 close_slot 释借 | 中 | 中 | 竞态 | 否 | dup3/pipe2 错误路径 |
| L-11 flush_all 释锁 + guard | 中 | 中 | RefCell | 否 | 全局 sync |
| L-12 socket_fd lazy/单表 | 中 | 中 | 旁路表同步 | 建议 | socket fork |
| L-13 check_nofile 缓存 rlimit | 中 | 低中 | setrlimit 失效 | 否 | open + setrlimit |
| L-14 execve 侧车表清理 | 中 | 低 | 纯加清理 | 否 | execve |
| L-15 clone 事务化 | 中 | 中 | 回滚一致 | 建议 | fork 失败注入 |
| L-16 ASID 回收 | 低 | 中 | TLB 一致性（同 M-2） | 是 | 长跑 + 回绕 |
| L-17 cwd/cred 索引 + cred 去 panic | 低 | 低 | 行为保持 | 否 | 无 |
| L-18 purge defer + guard | 低中 | 中 | RefCell | 否 | 批量 close |
| L-19 BufferedFileHandle dup 共享 | 低 | 低中 | dup 语义 | 否 | fork |
| L-20 设备表/inode/BAR 回收 | 低 | 低中 | 回收语义 | 否 | 长跑 |

## 建议排期（高收益 × 低风险优先）

1. **先吃低风险高收益**：H-3、F-9、M-8、F-4（配断言）、F-14、I-13。
2. **修 bug 类**：F-2、F-6、F-7、I-10、I-12、M-18（均直接降低现存风险）。
3. **中风险高收益（配断言/Flag）**：F-3、F-5、F-8、L-4、L-5、L-6、I-3、I-4、H-2、H-6/I-8。
4. **高风险高收益（最后、Flag+灰度+增量）**：M-1、M-2、H-4、H-1、M-3、M-5、L-1、L-3、I-2、H-14。

## 后续维护入口

- 本文件随实施进度更新「风险」「Flag」「验证」结论；与各子系统文档「风险与验证速查」表保持一致。
- 锁序/RefCell 相关结论同步 `docs/audits/locks/*`；TLB/ASID 同步两架构 `arch-impl` 与 `perf-memory.md`/`perf-hotpath.md`。
