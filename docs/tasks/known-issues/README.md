# WaterOS 已知问题任务索引

## 范围与口径

本目录是当前问题的统一执行入口，不按成员划分。状态依据 2026-07-29 的 `main`
（`b255b18e`）、工作树源码和现有运行报告整理。旧报告中的结论若已被代码实现取代，
标为“待复验”，不能再次照旧实现。

任务分为三类：

- **确认未闭环**：已有源码或最新日志证据，直接执行。
- **已实现待复验**：先跑指定测试；只有仍失败才改代码。
- **测量后候选**：没有阶段 1 数据不得合入策略性优化。

所有任务必须先读 `docs/prompts/general.md`、`docs/prompts/structure.md`、
`docs/prompts/coding.md` 和 `docs/prompts/architecture.md`。公共契约放 `api-v0`，
机制放 `impl-*`，平台特殊逻辑留在 `platform-impl`，架构通用 trap/启动逻辑留在
`platform-arch`。不得顺手修改 `os/vendor/`。

## 当前事实

- CAgent 已有连续三轮 10/10 记录，但最终候选版本仍须复验。
- Cargo 大索引读取根因已确认：大于 4 MiB 的 `read(2)` 被错误拒绝；短读修复尚在
  当前工作树，完整 BuildStorm 仍未输出 `BUILDSTORM_COMPILE mode=multi ok=true`。
- another-ext4 已通过基本读写和干净镜像完整性检查；压力场景仍需用写回、rename、
  unlink、truncate 与掉电边界测试证明。
- 双架构已有 SMP/IPI 实现：RISC-V 使用 SBI HSM/IPI，LoongArch 使用 IOCSR。
  “没有 IPI 实现”是过期结论；双架构 8 核运行门禁仍未完整记录。
- block cache 已在 RV/LA feature 中启用，容量为 1024 块；网络 RX/TX 已扩到 64 KiB
  且支持批量收包；ELF lazy map 和 TLSF 可选后端也已存在。这些任务从测量开始。
- ramfs 已支持稀疏文件，但已写入数据页仍是 `BTreeMap<u64, Vec<u8>>`，
  bootstrap `/tmp` 也没有容量上限。大量临时文件会消耗 128 MiB 全局内核堆；
  这是 [`K-05D`](./05d-ramfs-physical-pages.md) 的资源正确性问题，不等待 K-04 性能排名。

## 最近已验证闭环（2026-08-08）

- [`generic ABI epoll_pwait 修复`](./results/generic-abi-epoll-pwait-20260808.md)：
  RISC-V/LoongArch 的 `__NR_epoll_pwait` 修正为 22，`epoll_pwait04` 正确返回
  `EFAULT`，`epoll_wait01/02` 同步通过。
- [`epoll ctl/wait 语义修复`](./results/epoll-semantics-ctl-wait-20260808.md)：
  `epoll_ctl02/03`、`epoll_wait03`、`EPOLLRDHUP`、`EPOLLET` 与
  `EPOLLONESHOT` 定向用例全部通过。
- [`generic syscall 号与 siginfo_t 布局修复`](./results/generic-syscall-waitid-siginfo-20260808.md)：
  `waitid` 修正为 95，`poll` 不再占用 271；RISC-V `siginfo_t` pad 补齐后
  `waitid05/06` 通过。
- [`socketpair 未对齐指针校验`](./results/socketpair-unaligned-pointer-20260808.md)：
  `socketpair01` 的未对齐 `sv` 指针正确返回 `EFAULT`，定向用例全部通过。
- [`rmdir 错误语义修复`](./results/rmdir-enotempty-symlink-mount-20260808.md)：
  `rmdir02` 的 `ENOTEMPTY`、`ELOOP`、`EBUSY`、`EINVAL` 路径全部通过。
- [`listen UDP EOPNOTSUPP`](./results/listen-udp-eopnotsupp-20260808.md)：
  `listen01` 对 UDP socket 正确返回 `EOPNOTSUPP`。
- [`user-copy 无效地址 EFAULT`](./results/user-copy-invalid-address-efault-20260808.md)：
  坏指针路径不再误映射为 `EINVAL`，`statfs02/statfs02_64` 通过。
- [`readlink 父目录搜索权限`](./results/readlink-parent-search-permission-20260808.md)：
  `readlink03` 的 `EACCES` 路径正确返回，8 项全部通过。
- [`utimensat NULL pathname 与只读挂载`](./results/utimens-efault-20260808.md)：
  `utimes01` 的 `EFAULT` 与 `EROFS` 路径全部通过。
- [`select/pselect 无效 fd EBADF`](./results/pselect-invalid-fd-ebadf-20260808.md)：
  `pselect02/02_64` 的已关闭 fd 正确返回 `EBADF`。
- [`stat/statx 父目录搜索权限`](./results/stat-parent-search-permission-20260808.md)：
  `stat03/03_64` 的 `EACCES` 路径正确返回。
- [`mkdirat/mknodat 符号链接循环`](./results/mkdirat-symlink-loop-20260808.md)：
  `mkdirat02` 的 `ELOOP` 路径正确返回。
- [`statfs 父目录权限顺序`](./results/statfs-parent-search-order-20260808.md)：
  `pathconf02` 的 `EACCES` 路径正确返回。
- [`madvise 未实现 advice`](./results/madvise-unsupported-advice-20260808.md)：
  `madvise02` 的未实现 advice 正确返回 `EINVAL`，失败数从 10 降到 5。
- [`madvise VMA/映射范围检查`](./results/madvise-vma-range-check-20260808.md)：
  `madvise02` 的 `ENOMEM`/`EINVAL` 路径全部通过。
- [`waitpid 无效进程组 ESRCH`](./results/waitpid-invalid-pgid-esrch-20260808.md)：
  `waitpid04` 的 `INT_MIN` 进程组正确返回 `ESRCH`。
- [`sched_setaffinity 空 mask`](./results/sched-setaffinity-empty-mask-20260808.md)：
  `sched_setaffinity01` 的空 CPU mask 正确返回 `EINVAL`。
- [`sched_setaffinity EPERM`](./results/sched-setaffinity-eperm-20260808.md)：
  `sched_setaffinity01` 的降权调用正确返回 `EPERM`，四项错误语义全部通过。
- [`epoll_pwait2 syscall`](./results/epoll-pwait2-20260808.md)：
  `__NR_epoll_pwait2=441` 已实现，`epoll_pwait02..05` 的 pwait2 变体通过。
- [`mmap O_WRONLY fd`](./results/mmap-wronly-fd-eacces-20260808.md)：
  `mmap06` 的 O_WRONLY fd 映射正确返回 `EACCES`。
- [`MAP_SHARED_VALIDATE flag`](./results/mmap-shared-validate-flags-20260808.md)：
  `mmap20` 的非法 `MAP_SHARED_VALIDATE` flag 正确返回 `EOPNOTSUPP`。
- [`pwrite O_APPEND`](./results/pwrite-oappend-20260808.md)：
  `pwrite04/04_64` 的 O_APPEND 追加语义正确。

- [`RISC-V sscratch 切换修复`](./results/riscv64-sscratch-switch-20260808.md)：
  协作式上下文切换进入内核/idle 任务时清理 `sscratch`，消除“内核任务被误判为用户态
  trap”导致的 restore 失败。
- [`RISC-V scheduler 负载均衡修复`](./results/riscv64-scheduler-load-balance-mismatch-20260808.md)：
  禁用空闲偷取与亲和性放宽，恢复本地 runqueue 选择；完整 RISC-V Final BuildStorm
  重新输出 `BUILDSTORM_COMPILE ok=true`。

- [`K-25`](./results/k25-sched-getaffinity-cpusetsize-20260806.md)：
  `sched_getaffinity` 接受大 `cpusetsize`，guest `nproc` 从 1 恢复为 8。
- [`K-26`](./results/k26-exit-group-exiting-trap-boundary-20260806.md)：
  trap 返回用户态前处理 `ProcessState::Exiting`，修复多线程 `exit_group`
  后父进程 wait 永久等待，完整 Final 正常输出结果。
- [`K-27`](./results/k27-rust-parallelism-evidence-20260806.md)：
  Rust `available_parallelism()` 实测为 8，确认不是 Cargo job 数量被限制。
- [`K-28`](./results/k28-fd-registry-free-list-20260806.md)：
  fd registry 使用空闲集合与增量 open 计数，消除 O(N²) 路径。
- [`K-29`](./results/k29-unix-sock-owner-range-20260806.md)：
  AF_UNIX fork/exit 清理按 owner range 查询，不再扫描全局表。
- [`K-30`](./results/k30-block-cache-set-associative-index-20260806.md)：
  block cache 使用 8 路组相联 LBA 索引，完整 Final 记录 `1365.70s`。
- [`K-31`](./results/k31-block-cache-hit-run-lru-20260806.md)：
  block cache 连续命中区间批量拷贝并只刷新一次 LRU，完整 Final 可跑通。
- [`K-32`](./results/k32-block-cache-miss-run-insert-20260807.md)：
  block cache 连续 miss 区间直接插入，避免二次索引查找，完整 Final 可跑通。
- [`K-33`](./results/k33-paged-handle-size-cache-20260807.md)：
  PagedFileHandle 读路径不再逐次锁 ext4 metadata，完整 Final `1873.87s`。
- [`K-35`](./results/k35-page-cache-key-reuse-20260807.md)：
  页缓存 read/write 复用 FileCacheKey，降低 TLSF 分配热路径，完整 Final 通过。
- [`K-36`](./results/k36-page-cache-close-no-purge-20260807.md)：
  最后 close 不再立即 purge 页缓存，`purge_closed_file` 热点大幅下降。
- [`K-37`](./results/k37-pcore-affinity-20260807.md)：
  Final 运行默认绑定 P-core，完整 Final `elapsed_s=1348.86`。
- [`K-38`](./results/k38-page-cache-capacity-32mib-20260807.md)：
  页缓存扩容到 32MiB，完整 Final `elapsed_s=1282.12`。
- [`K-50`](./results/k50-procfs-read-range-20260807.md)：
  procfs 增加 range 读取，完整 Final `elapsed_s=1281.26`。
- [`回归汇总`](./results/regression-known-issues-20260807.md)：
  已知问题回归：RV Final/Pre、read-family、iozone、并行探针通过；LA Final 仍受
  `cargo xtask` 偶发竞态影响，重跑已通过。

## 2026-08-07 wait-hot 采样后新增任务

- [`K-53..K-58`](./11-buildstorm-performance-analysis.md)：修复 `cargo xtask` 返回
  竞态，并验证 `mprotect`、调度负载均衡、内存拷贝、VirtIO/block、TLSF 热点。
  完整分析见
  [`waithot-full-analysis-20260807.md`](../perf/waithot-full-analysis-20260807.md)。

以上组合完整 Final 可跑通。当前最优 `elapsed_s=1281.26`；K-31 完整轮在宿主高负载
下为 `1941.42`，K-32 低负载完整轮为 `1957.45`，K-33 为 `1873.87`，K-35 为
`1896.21`，K-36 为 `1881.13`；P-core 亲和性下为 `1348.86`，仍未达到 700-800s
目标。

## 必须同做与并行关系

```text
K-01 BuildStorm + fsync + 写回一致性 ─┐
RIO-01..10 read 调用族 ───────────────┼─> K-04 固定基线与测量
K-02 双架构 SMP/LA-musl ──────────────┤
K-03 功能性 0 分项 ──────────────────┘

K-04 ─┬─> K-05 FS/VFS 性能
      ├─> K-06 task/scheduler/futex
      ├─> K-07 MM/exec/fork/heap
      └─> K-08 网络吞吐

K-04 + K-06 + K-07 ─> K-09 trap/TLB 高风险优化
全部保留改动 ───────> K-10 最终回归与交付
```

K-01 内的 `fsync`、page-cache writeback、unlink/rename 失效和 another-ext4 flush 使用
同一持久化契约，必须作为一个闭环设计。RIO 内部依赖见
[`read-family/README.md`](../read-family/README.md)。K-05 至 K-08 可并行，但阶段 1
最多选择三个有数据支持的瓶颈。

## 任务清单

| 状态 | 任务 | 类型 | 交付 |
|---|---|---|---|
| [ ] | [`K-01`](./01-buildstorm-fs-durability.md) | 确认未闭环 | BuildStorm、fsync、时间戳、写回一致性 |
| [ ] | [`RIO-01..10`](../read-family/README.md) | 确认未闭环 | read/readv/pread、MM copy、OFD、读取源 |
| [ ] | [`K-02`](./02-smp-loongarch-validation.md) | 待复验 | RV/LA 8 核、IPI、LA-musl LTP |
| [ ] | [`K-03`](./03-functional-zero-scores.md) | 待复验 | regex、Pagefaults、busybox 0 分项 |
| [ ] | [`K-04`](./04-baseline-and-instrumentation.md) | 确认未完成 | Linux/WaterOS 三轮基线与 Top 3 |
| [ ] | [`K-05`](./05-fs-vfs-performance.md) | 混合 | dcache、页缓存 LRU、读写放大、ramfs 物理页 |
| [ ] | [`K-06`](./06-task-scheduler-futex.md) | 测量后候选 | ctx、队列、futex、退出生命周期 |
| [ ] | [`K-07`](./07-mm-exec-fork-heap.md) | 测量后候选 | lazy ELF、fork/COW、回收、allocator |
| [ ] | [`K-08`](./08-network-throughput.md) | 测量后候选 | poll、锁、收发批处理、缓冲 |
| [ ] | [`K-09`](./09-trap-tlb-hotpath.md) | 高风险候选 | trap、TLB/ASID、user-copy walk |
| [ ] | [`K-10`](./10-final-regression-delivery.md) | 最终门禁 | 双架构功能、性能、镜像与文档 |

K-02、K-03、K-05、K-06、K-07 是共享契约和合并门禁，实际分派使用下列可并行 leaf：

| 并行组 | 独立任务文件 |
|---|---|
| K-02 | [`02A SMP/IPI`](./02a-smp-ipi-runtime.md)、[`02B LA-musl`](./02b-loongarch-musl-ltp.md) |
| K-03 | [`03A regex`](./03a-regex-zero-score.md)、[`03B Pagefaults`](./03b-musl-rv-pagefault.md)、[`03C busybox`](./03c-busybox-kill-mv-rmdir.md) |
| K-05 | [`05A inode/dcache`](./05a-inode-dentry-cache.md)、[`05B page LRU`](./05b-page-cache-lru.md)、[`05C I/O/prefetch`](./05c-io-merge-prefetch.md)、[`05D ramfs 物理页`](./05d-ramfs-physical-pages.md) |
| K-06 | [`06A scheduler`](./06a-scheduler-ctx.md)、[`06B futex`](./06b-futex-waitqueue.md)、[`06C reap`](./06c-process-reap-lifecycle.md) |
| K-07 | [`07A lazy ELF`](./07a-elf-lazy-map.md)、[`07B fork/page table`](./07b-fork-pagetable-lifecycle.md)、[`07C heap`](./07c-kernel-heap-backend.md) |

K-07B 依赖 K-06C 的 retired-process 接口，不能同时修改生命周期 API；K-05A 与 K-05B
可并行，但必须先冻结 file identity/cache key。其余同一行 leaf 可使用独立 worktree
并行，最终按父任务验收合并。K-05D 可立即启动，但页所有权契约需与 K-07
协调，不允许让 FS 依赖某个架构的 MM 实现。

## 完整问题映射

| 既有编号/来源 | 新入口 |
|---|---|
| BuildStorm 完整编译、`fsync`、时间戳、F-2/F-6/F-7 | K-01 |
| read-family R1-R13、I-6、部分 L-3 | RIO-01..10 |
| 决赛 SMP、OpenSBI/IOCSR、G1 LA-musl LTP | K-02 |
| G4 regex、G5 Pagefaults、G9 kill/mv/rmdir | K-03 |
| 阶段 1 计数器和可比基线 | K-04 |
| G2/G6、F-1..F-21 中尚有数据支持的项 | K-05 |
| G3、I-1..I-17、L-1..L-20 的 task/IPC 部分 | K-06 |
| M-3..M-20、fork/exec/heap、L-1 的 MM 部分 | K-07 |
| G8 与 network/poll 相关项 | K-08 |
| G7、H-1/H-2/H-4/H-6、M-1/M-2 | K-09 |
| 阶段 3、CAgent、BuildStorm、LTP、文件系统完整性 | K-10 |

未被阶段 1 选中的 H/M/F/I/L 长尾项继续保留在 `docs/todo/perf-*.md`，不视为已经
批准实施。发现新问题时先补最新日志、最小复现和依赖边，再决定归入现有任务或新增
任务，不能直接替换当前 P0。

## 通用验收规则

- 构建至少执行 `cd os && make rv_check && make la_check`。
- 行为变更须在 RISC-V64 和 LoongArch64 验证；确实缺镜像时记录为阻断，不能用另一
  架构代替。
- 每轮 QEMU 使用独立 qcow2 overlay，原始镜像前后 SHA-256 不变。
- FS 写测试结束后将 overlay 转 raw，运行 `e2fsck -fn` 并保留五阶段结果。
- 性能数据至少三轮，保留单轮值、中位数、QEMU 参数、commit 和原始日志路径。
- 临时高频日志、kernel、overlay、镜像和大日志不得提交。
