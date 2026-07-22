# 决赛测例适配待办

## 范围和结论

本文按 `final_test_case/README.md` 和目录内两份测试脚本检查当前内核。检查日期为 2026-07-20，结论来自静态代码审查，尚未使用主办方提供的 Debian、Rust 工具链磁盘镜像做端到端运行。

当前内核不能直接通过 BuildStorm。最先阻断测试的项目有：两个架构都没有可运行的 8 核 SMP、`/proc/uptime` 缺失、`/proc/cpuinfo` 固定只报告 CPU 0，以及 QEMU 启动脚本固定为 1 核和 1 GiB。CAgent 的普通文件操作有实现基础，但 `/proc/net/tcp` 缺失，10 个子项也都需要在官方 glibc 镜像上确认。

状态标记：

- `[ ]` 未完成或没有运行证据
- `[~]` 有部分实现，仍不满足决赛验收
- `[x]` 已有实现且已有对应验证记录

## P0：先建立可重复的决赛测试入口

- [ ] **F0.0 恢复 LoongArch64 构建。** 2026-07-20 执行 `make la_check` 在 `platform-impl/impl-qemu-loongarch64-virt/src/lib.rs:88` 失败：实现仍提供 `time_frequency_hz`，`PlatformTime` trait 当前要求 `get_time_frequency_hz`。先修复接口漂移并让 `make la_check` 通过。同期 `make rv_check` 已通过，但有现存 warning。
- [ ] **F0.1 获取并校验双架构官方镜像。** 记录镜像哈希、QEMU 版本、主机物理核数和 Linux 基线 B。仓库只有测试源码，没有 README 所述的完整 Debian、Rust 工具链磁盘镜像，因此本次不能判定 `rustc` 和 `cargo` 的真实运行结果。
- [ ] **F0.2 增加决赛专用 QEMU 启动目标。** RISC-V64 和 LoongArch64 都必须使用 `-smp 8 -m 8G`，挂载官方镜像并保留串口日志。现有 `os/scripts/rv_qemu_run.sh:9`、`os/Makefile:153` 和 `os/scripts/la_qemu_run_snapshot.sh:20` 都固定为 `-smp 1`，内存为 1 GiB。
- [ ] **F0.3 建立结果归档。** 每次保存 commit、配置、完整串口日志、judge JSON、`TOOLCHAIN/MINIBUILD/COMPILE` 结果、编译耗时和产物大小。禁止通过修改时钟或 procfs 输出压低计时。
- [ ] **F0.4 先跑单核功能基线。** 在 SMP 改造前用同一镜像运行 CAgent、`rustc --version`、`cargo --version` 和最小工程，分开记录 Linux ABI 缺口与 SMP 引入的回归。

验收：两个架构都有一条可复制的命令，QEMU 日志能证明 8 个 CPU 在线、内存为 8 GiB，judge 可直接读取脚本输出。

## P0：实现双架构 8 核 SMP

完整的改造位置见 [`../单核设计转多核设计需要更改的位置的预记录.md`](../单核设计转多核设计需要更改的位置的预记录.md)。以下项目全部完成前，不应仅把 QEMU 的 `-smp` 改成 8。

- [ ] **S0.1 拆分 BSP 与 AP 启动路径，并提供每核 boot stack。** 当前 RISC-V `_start.S:8` 和 LoongArch64 `_start.S:12` 都无条件使用同一个 `boot_stack_top`。RISC-V 次核在 `os/src/main.rs:142-145` 永久 WFI；LoongArch64 没有 AP 分支。
- [ ] **S0.2 补齐架构和固件启动接口。** RISC-V 需要 OpenSBI HSM hart start/status，LoongArch64 需要按 QEMU virt 固件交接方式唤醒次核。两边都要提供 `current_cpu_id()`、CPU 上限和在线 CPU 集合。
- [ ] **S0.3 把调度器改成 SMP 安全。** 当前 round-robin 和 multi-class 的全局调度器都是 `UniprocessorSafeCell`，只有关闭本核中断的保护。需要全局 ready/wait 队列锁、per-CPU current/idle 状态、任务 Running(cpu) 所有权，以及不持调度锁跨越 `__switch` 的约束。
- [ ] **S0.4 替换跨核不安全的全局注册表。** 至少包括 process registry、frame allocator、fd、cwd、mount namespace 和 credential registry。`UniprocessorSafeCell` 在 `wateros-base/src/sync/uniprocessor.rs:16-17` 通过不安全 `Sync` 依赖单核约束，跨核并发会制造多个可变借用或 panic。
- [ ] **S0.5 修复内核堆的跨核递归误判。** `HEAP_GUARD_DEPTH` 是全局原子，核 B 会把核 A 的正常分配误判为递归分配。改成 per-CPU depth，并确认 TLSF/linked-list 后端的锁与中断顺序。
- [ ] **S0.6 每核初始化 trap、timer 和内核地址空间。** 每个 AP 都要设置 trap CSR、内核页表、timer deadline 和本核中断状态。RISC-V 的 `sscratch`/`tp` 与 LoongArch64 的相应 CSR 不能共享软件 current-task 状态。
- [ ] **S0.7 实现跨核 TLB 一致性。** `mmap`、`munmap`、`mprotect`、`brk`、exec 和地址空间销毁后，需要远程 shootdown 或一个有证明的地址空间调度约束。仅本核 flush 在多线程 rustc 下会留下陈旧映射。
- [ ] **S0.8 处理设备和日志并发。** 串口、块设备、页缓存、ext4、小读缓存、网络栈和 socket 表需要核对锁覆盖与锁顺序。先保证正确，再用细粒度锁或分片降低 rustc 并行编译时的争用。
- [ ] **S0.9 双架构 SMP 压测。** 运行 8 核任务创建/退出、共享地址空间线程、futex、并发文件读写、并发 page fault、网络 loopback 和长时间调度测试，并保留死锁与任务重复运行断言。

验收：8 个 CPU 都进入调度；同一 task 不会同时在两核运行；pthread/futex 压测、并发 mmap 和并发文件写入稳定；两架构都能连续运行 BuildStorm 级负载。

## P0：补齐 BuildStorm 的明确功能阻断

- [ ] **B0.1 实现真实 `/proc/uptime`。** 测试脚本用它计算 `T0` 和 `T1`。当前 `ProcNode` 只有 meminfo、cpuinfo、cgroups、mounts 和 pid 子树，见 `fs-procfs/procfs-impl/impl-kernel/src/lib.rs:68-84`。输出应由单调启动时钟生成，格式为 `<uptime_seconds> <idle_seconds>\n`，不得受墙上时钟调整影响。
- [ ] **B0.2 正确报告在线 CPU。** `format_cpuinfo()` 在同文件 `202-204` 固定只输出 `processor: 0`。`sched_getaffinity` 也必须按在线 CPU 填 mask，`nproc` 在两个架构都应返回 8，而不是硬编码 8。
- [ ] **B0.3 核对 8 GiB 物理内存边界。** LoongArch 启动在 `os/src/main.rs:335` 把可用 frame 上限截到 4 GiB。必须确认 QEMU 地址布局、direct map、页表 PPN 宽度、virtio DMA 和 frame allocator 可以使用评测要求的内存，或明确记录可用上限对 BuildStorm 是否足够。
- [ ] **B0.4 跑通动态链接器和 glibc TLS。** 分别运行 `ld.so`、`rustc --version` 和 `cargo --version`，核对 ELF interpreter、TLS、`arch_prctl` 或架构等价接口、auxv、vDSO/clock、`mmap`、`mprotect`、signals 和线程退出。
- [ ] **B0.5 跑通 cargo minibuild。** 覆盖目录创建、临时文件、rename/link/symlink、stat/statx、`fcntl` 锁、pipe、poll/epoll、fork/exec/wait、futex、随机数和 linker 子进程。现有 syscall 实现只能算候选基础，必须以工具链日志中第一个失败 syscall 为准迭代。
- [ ] **B0.6 修正伪文件系统挂载兼容性。** 测试会执行 proc、sysfs 和 devtmpfs mount。当前 procfs 有实现，tmpfs 主路径也已落地，但需要确认 `mount -t sysfs`、`mount -t devtmpfs` 的返回值和 `/dev/null`、`/dev/urandom`、`/dev/tty` 等节点满足工具链。关联待办见 [`fs-ramfs-tmpfs.md`](./fs-ramfs-tmpfs.md)。
- [ ] **B0.7 验证大型 ext4 工作集的正确性。** 对数百 crate 并发构建验证 page cache 写回、truncate、rename、unlink、fsync、目录遍历和 mmap 文件一致性。构建成功后再处理缓存容量与锁竞争。

验收：两个架构都输出 `BUILDSTORM_TOOLCHAIN ok` 和 `BUILDSTORM_MINIBUILD ok`，且 `/proc/uptime` 单调、`nproc` 为 8。

## P1：CAgent 十项适配

- [~] **C1.1 factorial。** shell、算术和基础进程执行已有 BusyBox bring-up 基础；用官方 `agent_lite` 实测输出和超时。
- [~] **C1.2 date。** 内核有 wall clock 和 clock syscall；需要验证 RTC、时区环境、`date -d '100 days ago'` 涉及的 glibc 时间接口。
- [ ] **C1.3 network。** 增加 `/proc/net/tcp` 和必要的 `/proc/net` 目录，内容来自 socket 元数据，至少能让 `ss`/`netstat` 或 agent 选择的命令统计 ESTABLISHED。当前 procfs 没有 net 子树。
- [~] **C1.4 cpu。** 依赖 B0.2；CAgent 只要求数字，BuildStorm 则要求真实 8 核。
- [~] **C1.5 kernel。** `uname` syscall 已实现；确认 release 字段含可解析的 `x.y` 版本号。
- [~] **C1.6 fs-create。** 已有 open/create/write/read 基础；检查工作目录写权限、truncate 和 close 后可见性。
- [~] **C1.7 fs-readwrite。** 验证 shell 重定向、顺序读写、文本工具和并发测试间文件名隔离。
- [~] **C1.8 fs-directory。** 验证 mkdir、getdents64、目录项计数和删除。
- [~] **C1.9 fs-search。** 验证递归 find 所需的 lstat/newfstatat、getdents64、路径规范化和深目录性能。
- [~] **C1.10 fs-usage。** `statfs/fstatfs` 已有 syscall 实现；确认块数、单位、挂载点和 `df -h` 输出不是零值或溢出。
- [ ] **C1.11 验证测试脚本并发。** 十项都在后台同时运行，另有 `simple_llm_server` 和大量 loopback TCP。测试 fd/process 上限、accept/poll、SIGCHLD、timeout/kill、`wait`、`/tmp` 唯一文件和清理路径。

验收：两个架构的 10 项均为 pass；先追求无超时正确性，再以每项 50% 超时线为目标优化。

## P2：BuildStorm 成功后的性能工作

- [ ] **P2.1 建立阶段计时。** 分开统计 cargo 元数据、rustc 前端、代码生成、链接、文件 IO、page fault、上下文切换和锁等待，不用串口日志量代替 profiling。
- [ ] **P2.2 降低全局调度锁争用。** 正确性版本可用全局队列，性能版本应评估 per-CPU run queue、负载均衡、唤醒目标和减少 idle 核空转。
- [ ] **P2.3 优化文件与块 IO。** 复用现有 page cache 和 block cache 分析，重点测并发小文件、metadata lookup、dirty writeback、LRU 和 ext4 全局锁。关联文档为 [`perf-fs-vfs.md`](./perf-fs-vfs.md)。
- [ ] **P2.4 优化内存和进程路径。** 测量 mmap/munmap、page fault、frame allocator、fork/exec/exit 和内核堆争用。关联文档为 [`perf-memory.md`](./perf-memory.md) 与 [`perf-fork-exit-degradation.md`](./perf-fork-exit-degradation.md)。
- [ ] **P2.5 优化 futex、pipe、poll 和 epoll。** rustc/cargo 多线程和子进程会放大惊群、全表扫描与粗锁。关联文档为 [`perf-ipc-sync.md`](./perf-ipc-sync.md)。
- [ ] **P2.6 控制日志。** release 构建关闭 syscall/trap 高频日志，避免串口成为编译瓶颈，同时保留 panic、OOM 和最终判题标记。

验收：BuildStorm 连续运行至少 3 次成功，记录中位数与波动；再与同机 Linux 基线 B 比较，所有优化都附修改前后数据。

## P3：提交材料

- [ ] **D3.1 编写内核设计与优化文档。** 内容必须包含问题定位、根因、实现方案、修改前后时间和加速比。
- [ ] **D3.2 记录 AI 使用。** 保存任务、输入、人工复核点、最终 diff 和可复现命令，不把未经运行证实的静态推断写成实验结论。
- [ ] **D3.3 提供一键复现步骤。** 从镜像校验、内核构建、双架构 QEMU 参数到 judge 命令均可由审核者执行。

## 推荐执行顺序

1. F0 测试入口和单核基线。
2. B0.1、B0.2、B0.6 等不依赖 SMP 的明确功能缺口。
3. S0 双架构 SMP，先正确性后扩展性。
4. B0 工具链和 minibuild，按首个失败点迭代。
5. C1 十项并发回归。
6. BuildStorm 全量构建与 P2 性能优化。
7. D3 文档和复现材料。

## 本次检查边界

- 没有主办方完整磁盘镜像，未执行 `rustc`、`cargo`、CAgent 或 BuildStorm。
- 没有在本次检查中修改内核代码，也没有把现有 syscall 清单等同于 glibc 兼容性证明。静态构建基线为 RISC-V64 通过、LoongArch64 因 F0.0 所述 trait 接口漂移失败。
- 文档中的行号用于定位本次审查证据。实施时应以符号名搜索，避免后续代码移动造成误导。
