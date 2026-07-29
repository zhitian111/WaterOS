# K-02：双架构 SMP、IPI 与 LoongArch-musl 验证

## 任务目标

证明 RISC-V64/OpenSBI 与 LoongArch64/QEMU 都能以 8 核运行启动、定时器、远端唤醒、
TLB shootdown 和最终测例；同时确认旧评分中 LoongArch-musl LTP 整套 0 分是否仍可
复现并修复首个根因。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-platform.md`
- `docs/exports/features/wateros-task.md`
- `docs/exports/features/wateros-mm.md`
- `docs/单核设计转多核设计需要更改的位置的预记录.md`
- `docs/tasks/run_testsuits_qemu.md`
- `docs/todo/perf-baseline-gap-report.md`

## 已知信息与代码证据

“OpenSBI 无法多核”和“LoongArch 没有 IPI”已经不是当前源码状态。平台层已有：

```rust
pub fn send_ipi(mask: CpuMask, kind: IpiKind) -> PlatformSmpResult<()> {
    // 先记录软件原因，再调用 active platform 的硬件通知。
    crate::active_impl::smp::SmpImpl::send_ipi(mask)
}
```

RISC-V platform impl 使用 `sbi::hart_start`、`sbi::send_ipi` 和
`remote_sfence_vma`；LoongArch impl 使用 `IOCSR_IPI_SEND/STATUS/CLEAR`。scheduler
也要求 SBI/IOCSR 调用发生在 scheduler 锁外。

尚缺的是可追溯的双架构 8 核端到端记录。旧 score 还显示 LoongArch-musl LTP 整套
为 0，而 glibc-la 和 musl-rv 可运行；这条证据较旧，必须先复现。

## 涉及文件

- `os/src/main.rs`
- `os/src/trap_handler.rs`
- `os/components/wateros-platform/platform-api/api-v0/src/smp.rs`
- `os/components/wateros-platform/src/lib.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/lib.rs`
- `os/components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/lib.rs`
- `os/components/wateros-platform/platform-arch/arch-impl/impl-{riscv64,loongarch64}/`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/`
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/`
- `os/src/user_bringup_busybox.rs`
- `os/scripts/{rv,la}_final_run.sh`

## 可并行任务

- [`K-02A：双架构 8 核与 IPI`](./02a-smp-ipi-runtime.md)
- [`K-02B：LoongArch-musl LTP`](./02b-loongarch-musl-ltp.md)

两项可并行调查；K-02B 的最终完整 LTP 仍须在 K-02A 证明的 8 核启动环境复验。

## 任务内容

1. 固化两架构 QEMU 命令、固件版本、镜像 hash、`-smp 8 -m 8G` 和 online CPU 日志。
2. 验证 AP 启动栈、per-CPU CPU ID、timer、idle task 和 trap context 都是每核独立。
3. 用远端 wake、task notify 和强制 reschedule 探针验证 IPI 原因不会丢失、合并后仍
   会被处理，且 clear 顺序不会导致中断风暴。
4. 用跨核 `mmap/munmap/fork/exec` 探针验证 TLB shootdown；失败时先区分 IPI 未达、
   pending reason 丢失、地址空间 generation 错误和本地 flush 缺失。
5. 在 LoongArch-musl 上只运行最小 ELF、busybox 和单个 LTP 用例，再运行完整脚本。
   如果已经通过，只更新旧报告；如果失败，以第一条 exec/panic/mount 错误为根因。
6. 不要为通过测试恢复共享 boot 栈、固定 CPU 0 或本核关中断伪锁。

最小在线断言应比较配置与 scheduler 观察值：

```rust
let configured = platform::smp::configured_cpu_mask();
let online = task::online_cpu_mask();
assert_eq!(online & configured, configured);
```

正式代码应使用错误返回或带超时的启动门禁，不能无限等待 AP。

## 如何验收

- [ ] 两架构 `make *_check` 和 final kernel 构建通过。
- [ ] 两架构均报告 8 个 online CPU，`/proc/cpuinfo`、`nproc` 和 affinity 一致。
- [ ] 每个 CPU 都运行过用户 task、timer tick 和 idle 路径。
- [ ] 10,000 次跨核 wake/reschedule 无丢失唤醒、永久 idle 或 IPI storm。
- [ ] TLB shootdown 压测无旧映射、错页、UAF 或跨进程数据泄漏。
- [ ] LoongArch-musl LTP 得到有效结果；仍阻塞时报告精确复现和首个根因。
- [ ] SMP 测试结束无 scheduler 锁递归、持锁 SBI/IOCSR 调用和 panic。

结果写入 `docs/tasks/known-issues/results/k02-YYYYMMDD.md`。缺少 LoongArch final 镜像
属于明确阻断，必须记录镜像需求和已完成的构建/最小启动证据，不能标记任务完成。
