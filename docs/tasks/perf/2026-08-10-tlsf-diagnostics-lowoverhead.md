# TLSF 低开销诊断方案（2026-08-10）

## 为什么选择这里

当前 BuildStorm 基线的 allocator 相关热点来自 TLSF allocate/deallocate、allocator
guard 和 `___rust_alloc/dealloc`。交接文档明确要求：

1. 不要在没有真实 size 分布和锁竞争数据的情况下实现 per-CPU cache。
2. 现有 `tlsf-diagnostics` 草稿使用全局 `AtomicU64::fetch_add`，300 秒诊断连
   `BUILDSTORM_TOOLCHAIN` / `MINIBUILD` marker 都未到达，无法提供有效数据。

因此第一步不是优化 allocator，而是修复诊断基础设施。没有这一步，后续任何
allocator 方案都只能靠猜测，重复 `perf/tlsf-slab` 这类完整轮失败。

## 选择的方案

把 `tlsf_diagnostics.rs` 的全局原子计数改为 per-CPU 普通整数计数：

```rust
#[repr(C, align(64))]
struct PerCpuCounters {
    alloc: [u64; 9],
    free: [u64; 9],
    realloc: [u64; 9],
    alloc_bytes: [u64; 9],
    align_gt16: u64,
    lock_acquire: u64,
    lock_contended: u64,
    oom: u64,
}

static COUNTERS: CpuLocal<PerCpuCounters, MAX_CPUS> =
    CpuLocal::from_cells([const { UnsafeCell::new(PerCpuCounters::new()) }; MAX_CPUS]);
```

分配/释放/realloc 路径已经在 `with_allocator_interrupt_guard` 内运行，本地中断已关闭。
因此每个 CPU 只写自己的槽位，可以使用普通 load/store，不再执行跨 CPU 共享的
`AtomicU64::fetch_add`。输出阶段再汇总所有 CPU。

## 为什么这么做

- 全局 `AtomicU64` 的 locked RMW 在每个 alloc/dealloc 上产生跨核缓存行流量，是诊断
  明显拖慢系统的主要原因。
- `CpuLocal` 已经是仓库内现有基础设施，`interrupt_guard.rs` 的递归深度就在使用它。
- per-CPU 槽位按 64 字节对齐，降低伪共享。
- 普通 Final 通过 `cfg(feature = "tlsf-diagnostics")` 完全排除，不影响比赛产物。

## 接下来的工作

1. 在 `perf/tlsf-diagnostics-lowoverhead` 分支实现 per-CPU 计数。
2. `make check ARCH=rv PROFILE=final EXTRA_FEATURES=tlsf-diagnostics` 和 LA 同参数。
3. 普通 Final `make check ARCH=rv PROFILE=final` 确认无诊断符号。
4. 使用 `perf/tlsf-slab` 临时 worktree 的 `buildstorm_runner.py` 跑 300 秒诊断。
5. 检查 `BUILDSTORM_TOOLCHAIN` / `MINIBUILD` 与 `BUILDSTORM_PERF_COUNTERS`。
6. 记录 size bucket、lock acquire/contended、OOM，然后提交诊断分支。
7. 根据数据选择下一个 allocator 优化或调用方分配消除。

## 验收标准

- 双架构 `tlsf-diagnostics` check 通过。
- 普通 Final check 通过且产物不含 histogram/锁等待计数。
- 300 秒诊断能到达 `BUILDSTORM_MINIBUILD ok` 并输出 `BUILDSTORM_PERF_COUNTERS`。
- 无 panic、SIGSEGV、明显卡死。
