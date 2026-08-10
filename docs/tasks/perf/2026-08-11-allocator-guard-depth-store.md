# allocator guard 每 CPU 深度普通 load/store 方案（2026-08-11）

## 为什么选择这里

`guard-fastpath-full-a1/a2` 把 allocator guard 的 CSR disable/restore 快路径化后，
完整 BuildStorm 为 `867.22s / 868.62s`，但同一宿主 main 复测为 `874.46s`，净收益
只有约 0.8%，未达到 1.5% 合并门槛。guard 路径仍然每次执行：

```text
fetch_add(1, Acquire)
fetch_sub(1, Release)
```

`fetch_add` / `fetch_sub` 会生成带锁前缀或至少比普通 load/store 更重的原子 RMW。
该 depth 槽是每 CPU 的，且进入 `f()` 前全局中断已被关闭，因此本 CPU 不可能被
异步打断，其它 CPU 也永远不会写这个槽。

## 选择的方案

在 `runtime-heap-allocator/src/interrupt_guard.rs` 中保留 `CpuLocal<AtomicUsize>`
类型，但把深度维护从 `fetch_add` / `fetch_sub` 改为：

```rust
let depth = local_depth.load(Ordering::Relaxed);
if depth > 0 {
    panic!("recursive heap allocation detected ...");
}
local_depth.store(1, Ordering::Relaxed);
let ret = f();
local_depth.store(0, Ordering::Relaxed);
```

- 递归检测仍有效：进入 `f()` 前 depth 必须是 0；递归分配会读到 1 并 panic。
- 不引入无锁数据结构的新所有权问题：仍使用 `AtomicUsize`，只是去掉 RMW。
- 不改变跨 CPU 互斥：TLSF 后端的 `Mutex` 仍然是真正跨核保护。

## 为什么这么做

1. guard 已保证进入闭包前本 CPU 中断关闭，同一槽位不存在并发 writer。
2. `AtomicUsize::load/store` 仍保留原子变量的编译器可观测语义，但不会发出
   `amoadd`/`amoadd.w.aq` 这类 RMW 指令。
3. 与 `perf/tlsf-slab` 旧分支的同类改动一致，但本次从纯 main 出发、不叠加 slab，
   可以单独测量收益。

## 接下来的工作

1. 在 `perf/allocator-guard-depth` 分支基于 guard 快路径实现。
2. 双架构 Final `make check`。
3. 180 秒 smoke 确认递归检测和中断状态无回归。
4. 完整 RISC-V BuildStorm A/B，与同轮 main 复测比较；两轮净改善 ≥ 1.5% 才合并。

## 验收标准

- 双架构 Final check 通过。
- 普通 Final 无递归分配 panic、无中断永久关闭。
- 完整 BuildStorm 相对同宿主 main 有可复现 ≥ 1.5% 的净改善。

## 实测结果（2026-08-11）

```text
guard-depth-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=865.50
main-5a080c07-full-b1: BUILDSTORM_COMPILE ok=true elapsed_s=874.46
```

双架构 Final check 通过；180 秒 smoke 无 panic/SIGSEGV/stall。完整 BuildStorm
`865.50s` 相对同轮 main `874.46s` 快约 1.0%，仍未达到 1.5% 合并门槛。guard+depth
组合保留为实验分支，不回退 main，不作为当前最优候选。

300 秒 pc-hot 中 guard alloc/dealloc 路径指令分别为约 `627.7M / 469.5M`，相对
纯 main 采样略有下降，但完整墙钟收益仍被运行噪声和更上层瓶颈限制。
