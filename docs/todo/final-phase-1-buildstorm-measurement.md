# 阶段 1：BuildStorm 基线与测量

## 阶段目标

把“446 个单元编译较慢”拆成可量化问题，得到 Linux 与 WaterOS 可比基线、各阶段耗时
和瓶颈 Top 3。此阶段允许加入可关闭的低开销计数器，不做改变核心策略的优化。

## M1-1 固定实验口径

**负责人：A**

- 同一评测机、QEMU 版本、`-smp 8 -m 8G`、镜像和脚本分别运行 Linux 与 WaterOS。
- 每轮使用全新 overlay；预热策略、是否清 target、Cargo job 数必须一致。
- BuildStorm 完整运行至少三次，保留单轮值和中位数。
- 单独记录 toolchain、minibuild、`cargo metadata`、`tg-xtask` 和正式编译耗时。
- 保存 guest `/proc/uptime` 计时，同时记录宿主机墙钟时间，检查二者是否一致。

产出：`docs/results/buildstorm-baseline-YYYYMMDD.md` 和对应原始日志。

## M1-2 Task 与 CPU 利用率

**负责人：B**

以 per-CPU 累计计数器记录：

- running、idle 时间或 tick；
- context switch、wake、远端 reschedule IPI 数；
- runnable task 数量的采样分布；
- scheduler lock 获取次数和竞争次数；
- futex wait/wake/requeue 数及无 waiter 的 wake 次数。

计数器不得在热路径逐条打印。由 procfs/debug snapshot 在阶段结束时一次性导出。若
多数 CPU 长时间 idle 且存在 runnable task，优先处理调度；若 CPU 持续忙，则继续看
syscall、页 fault 和 I/O。

## M1-3 文件系统与内存路径

**负责人：A**

记录以下累计量和耗时桶：

- `read/write/pread/readv/writev/fsync/close` 次数、字节数和短读次数；
- 页缓存 hit/miss、脏页、预读命中、evict 和 writeback；
- 块缓存 hit/miss、设备读写块数；
- 路径解析和 `path_to_inode` 次数；
- minor/major page fault、用户页复制字节数；
- 内核堆分配失败及按大小区间的分配次数。

先使用累计计数和粗粒度时间桶。禁止在每次块 I/O 或 syscall 上写串口日志，否则测量
本身会改变结论。

## M1-4 网络回归侧基线

**负责人：C**

BuildStorm 不依赖外网，C 可并行建立 CAgent 基线：每项耗时、TCP connect/accept
次数、poll 次数、TX/RX 包数及 network stack 锁竞争。该数据用于保证后续内核通用
优化没有损害 CAgent，也为网络性能优化提供基线。

## 分析决策树

1. 编译单核忙：检查 Cargo jobserver、clone/futex/wake 和可运行队列。
2. 多核忙但吞吐低：检查 page fault、用户拷贝、exec 和编译器自身 CPU 开销。
3. 多核大面积 idle 且 I/O 活跃：检查 ext4 小块访问、缓存命中、flush 和锁等待。
4. 多核大面积 idle 且 I/O 不活跃：检查 futex/pipe/wait4 丢失唤醒或错误阻塞。
5. 每次重跑都全量编译：优先验证时间戳、fsync 和 Cargo freshness，不优化调度器。

## 阶段出口

- [ ] Linux 与 WaterOS 各有至少三轮可比结果。
- [ ] 计时口径和 judge 输出一致。
- [ ] 有按贡献排序的瓶颈 Top 3，每项都有计数或时间证据。
- [ ] 能说明低吞吐主要属于 CPU、调度等待、文件 I/O 还是错误重编。
- [ ] 诊断 feature 默认关闭，关闭后不改变功能结果。

出口评审只选择最多三个阶段 2 任务，避免同时修改调度、页缓存、块缓存和页表而失去
A/B 对比能力。
