# 阶段 2：性能优化实施

## 阶段目标

只实施阶段 1 数据支持的 Top 3 瓶颈。每项先测基线、单独修改、用相同负载复测，
再决定保留或回退。已有通用分析见
[`perf-risk-assessment.md`](./perf-risk-assessment.md) 和
[`../tasks/perf/README.md`](../tasks/perf/README.md)。

## 优化选择规则

| 证据 | 首选任务 | 负责人 |
|---|---|---|
| 块缓存 miss 和设备读块数高 | 调整/启用块缓存、顺序读合并 | A |
| 页缓存 miss/evict 或重复读高 | 页缓存容量、LRU、预读和 writeback | A |
| `path_to_inode` 占比高 | 受 mount generation 约束的 dentry/inode cache | A |
| 小 read/write syscall 数过高 | 向量 I/O、拷贝批量化、合理短读粒度 | A |
| runnable 存在但 CPU idle | 唤醒、IPI、调度队列修复 | B |
| scheduler lock 竞争高 | 缩短临界区，再评估 per-CPU queue | B |
| futex 等待占主要时间 | wait/wake/requeue 与退出清理优化 | B |
| exec/fork/exit 占比高 | 资源查找、销毁锁外化、地址空间路径 | A+B |
| CAgent 网络锁/poll 占比高 | poll 驱动、锁粒度、收发批处理 | C |

## O2-A 文件系统与内存

候选项按风险递增：

1. 调整现有 block/page cache 配置并验证双架构确实启用。
2. 合并连续块读写，减少 another-ext4 到 block device 的细粒度调用。
3. 优化顺序读预取和页缓存淘汰，保持脏页竞态测试。
4. 缓存路径到 inode 的解析结果，并以 mount generation、rename、unlink、truncate
   做完整失效。
5. 仅在证据明确时优化 exec 映射、page fault 和用户拷贝。

相关实施任务优先复用：

- `docs/tasks/perf/wave1-enable-block-cache.md`
- `docs/tasks/perf/wave2-fs-read-path.md`
- `docs/tasks/perf/wave2-execve-lazy-map.md`

高风险的 TLB/ASID、页表 COW 和 allocator 替换不与上述任务同批合入。

## O2-B Task 与同步

实施顺序：

1. 修正发现的错误唤醒、空闲 CPU 未通知或锁内阻塞。
2. 缩短 scheduler/process registry 临界区，把大对象 drop 移到锁外。
3. 只有全局 ready queue 竞争已被测量为主要瓶颈时，才评估 per-CPU run queue 和
   work stealing。
4. 保留 task 单一状态、单 CPU 所有权及 futex 无丢失唤醒断言。

B 修改 task API 时先交付接口提交，A 再修改 procfs、MM 或 syscall 调用方。

## O2-C 网络

C 以 CAgent 计时和网络计数为依据优化 poll、TCP 状态推进和锁粒度。BuildStorm 优化
期间继续运行 CAgent 快速回归；网络改动不能与文件系统或 scheduler 改动混成一个
结果样本。

## 单项闭环

每个优化必须完成：

1. 写下假设和预期影响的指标。
2. 在固定 commit 和镜像上取得修改前三轮数据。
3. 单独提交实现，运行静态检查和定向正确性测试。
4. 取得修改后三轮数据，报告中位数、波动和加速比。
5. 运行 BuildStorm、CAgent 及受影响的初赛回归。
6. 无稳定收益、收益小于噪声或引入回归时，不进入最终候选分支。

## 阶段出口

- [ ] BuildStorm 保持完整成功，且耗时相对阶段 1 基线稳定下降。
- [ ] 每个保留优化都有独立提交和修改前后数据。
- [ ] CAgent 仍连续三轮 10/10。
- [ ] 文件系统压力测试后 `e2fsck -fn` 通过。
- [ ] 无新增 panic、锁序告警、任务永久阻塞或跨核数据损坏。
- [ ] 高风险改动具备 feature 回退路径和定向断言。
