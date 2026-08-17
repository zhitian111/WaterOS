# 任务 04 简报：进程对象原子 IO 计数

## 完成状态

已完成。`rchar/wchar/syscr/syscw` 从 CWD registry 移入 PCB 持有的原子状态；成功的
read/write 系 syscall 不再进入 CWD registry 或查询其 owner/BTreeMap。

## 提交

本简报与 `[perf] 进程对象使用原子 IO 计数` 实现位于同一提交。

## 关键文件与行为

- `os/components/wateros-task/task-impl/impl-core/src/{process,lib}.rs`
  - PCB 持有 `Arc<ProcessIoCounters>`，四项计数用 relaxed `AtomicU64` 饱和累加。
  - 每 CPU 缓存当前 task 的计数器引用；命中只持本 CPU 短锁并更新原子。
  - cache 临界区禁用本核中断，避免持锁任务被切走后同核自旋。
  - fork 新计数归零，thread clone 共享 PCB/计数，exec 保留，reap 随 PCB 回收。
- `os/components/wateros-task/src/process.rs`
  - 提供计数与快照的 task 聚合接口。
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/lib.rs`
  - syscall 成功路径直接调用 task 计数接口。
- `os/components/wateros-vfs/{src/cwd.rs,src/lib.rs}` 与
  `vfs-impl/impl-fd-session/src/cwd.rs`
  - 删除 CWD owner 表中的 IO 计数及转发；procfs callback 改读 PCB 原子快照。
- `os/components/wateros-task/readme.md`
  - 同步计数的并发与 fork/clone/exec 生命周期契约。

任务切换后该 CPU 第一次计数仍需一次 process registry 查询以刷新 cache；同一调度片内后续
计数不进入任何全局 registry。

## 验证

通过：

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
cd ..
git diff --check
```

新增 `process_io_is_shared_by_threads_and_cleared_on_fork` 测试。host 定向测试命令：

```bash
cd os
cargo test --offline --manifest-path \
  components/wateros-task/task-impl/impl-core/Cargo.toml \
  --features arch/impl-riscv64 process_io_is_shared_by_threads_and_cleared_on_fork
```

未能运行，原因是 x86_64 host 无法编译 `sbi-rt` 中的 RISC-V `a0..a7` 寄存器汇编；两架构
内核检查已完成实现与调用方的目标编译验证。

## 性能与剩余风险

任务 00/01 尚未实现，未执行 QEMU BuildStorm A/B 或 cwd registry 进入次数采样。尚需在
QEMU 中验证 `/proc/self/io`、线程共享、fork 清零和 exec 保留。原子快照允许并发字段间存在
瞬时差异，符合统计文件的近似快照属性；计数采用饱和而非回绕。若 proc IO 值或生命周期语义
回归，应回退本提交。

## 文档同步

已同步 `os/components/wateros-task/readme.md`；未改变公开构建命令或 feature。
