# K-04：固定基线、低扰动测量与瓶颈排序

## 任务目标

在功能门禁通过后，建立 Linux 与 WaterOS 可比的三轮基线，把“BuildStorm 慢”和旧
性能 score 拆成有计数或时间证据的瓶颈 Top 3。此任务只测量，不改变调度、缓存、
页表或网络策略。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/snapshot/current.md`
- `docs/exports/architecture/components.md`
- `docs/exports/architecture/module-relations.md`
- `docs/tasks/run_testsuits_qemu.md`
- `docs/tasks/known-issues/README.md`

## 前置条件

K-01、K-02、K-03 和 RIO-01..10 已通过各自功能验收，或有会议确认的非阻断例外。
BuildStorm 必须完整成功；未成功时继续归入兼容性问题。

## 已知信息与代码证据

- 旧报告中的 block cache 未启用、RX/TX 仅 2 KiB、ELF eager load 等结论已经部分
  过期；当前源码分别显示 1024 块 cache、64 KiB network buffer 和 `elf-lazy-map`。
- 最新 BuildStorm 只证明进入 446 单元编译，尚无完整耗时基线。
- 高频串口日志会显著改变 syscall、I/O 和调度测量。计数器应使用原子/per-CPU 累计，
  结束时一次性 snapshot：

```rust
counter.fetch_add(1, Ordering::Relaxed);
// 在 procfs/debug snapshot 中统一读取，热路径不打印。
```

## 涉及文件

- `os/components/wateros-base/base-config/`
- `os/components/wateros-task/task-scheduler/`
- `os/components/wateros-vfs/vfs-impl/impl-{page-cache,fs-bridge}/`
- `os/components/wateros-driver/driver-{block,network}/`
- `os/components/wateros-mm/`
- `os/components/wateros-syscall/`
- `os/components/wateros-fs/fs-procfs/`
- `os/scripts/{rv,la}_final_run.sh`
- `final_test_case/README.md`
- `docs/tasks/run_testsuits_qemu.md`

## 任务内容

1. 固定评测机、QEMU、固件、commit、submodule、镜像 hash、`-smp 8 -m 8G`、Cargo
   jobs、target 清理和预热策略。
2. Linux 和 WaterOS 各运行至少三轮完整 BuildStorm，记录 host wall time 与 guest
   `/proc/uptime`，分解 toolchain、minibuild、metadata、xtask 和 compile。
3. 以 per-CPU 累计方式记录 running/idle、context switch、wake/IPI、runqueue、
   scheduler 锁和 futex wait/wake/requeue。
4. 记录 syscall 次数/字节/短读、page/block cache hit/miss、writeback、设备块数、
   path lookup、fault、user copy 和 heap size bucket。
5. 网络侧记录 poll、TX/RX frame、TCP/UDP 字节、全局 stack 锁等待与 CAgent 单项
   耗时。
6. 运行现有 score 测试，更新 LA-musl、ctx、regex、Pagefaults、busybox、iozone、
   lmbench 和 network 的当前结果。
7. 依据等待/CPU/I/O 决策树只选 Top 3；每项须有基线指标、预期改变指标和噪声范围。

计数器 API 应在组件内部导出结构化快照，不让 syscall 依赖实现内部全局变量。例如：

```rust
pub struct FsPerfSnapshot {
    pub page_hits: u64,
    pub page_misses: u64,
    pub block_reads: u64,
    pub writebacks: u64,
}
```

诊断 feature 默认关闭；关闭后内存布局和功能结果不能发生不必要变化。

## 如何验收

- [ ] Linux/WaterOS 至少各三轮完整 BuildStorm，保留单轮值和中位数。
- [ ] 所有结果可追溯到 commit、镜像、QEMU 命令和原始日志。
- [ ] guest uptime 与 host wall time 的差异已解释。
- [ ] 热路径没有逐 syscall、逐页、逐包日志。
- [ ] Top 3 每项都有至少一个直接计数或耗时证据，能归类为 CPU、调度等待、I/O、
      fault/copy 或网络。
- [ ] 诊断 feature 关闭后 BuildStorm、CAgent 和 basic/busybox 结果不变。
- [ ] 报告明确哪些旧问题已关闭、哪些仍复现，不把历史分析当当前测量。

结果写入 `docs/tasks/known-issues/results/k04-baseline-YYYYMMDD.md`。K-05 至 K-09 只能
选择该报告支持的任务。
