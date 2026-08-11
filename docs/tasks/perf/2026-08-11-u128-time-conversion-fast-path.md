# u128 时间换算快速路径（2026-08-11）

## 为什么选择这里

对 `main-809-pchot-full` 的 93.2B 指令采样做符号归并后，`compiler_builtins`
的 `u128_div_rem` 单独占了约 6.34B 指令，约为全部采样的 6.8%，是当前采样中
最大的可归属内核符号。

静态检查发现内核时间路径里有多个 u128 除法：

- `platform::timer::ticks_to_duration`：每次 `clock_gettime(CLOCK_MONOTONIC)`
  都需要把 RISC-V tick 转成纳秒；
- `sys/time/clock.rs::ns_to_timespec`：每次 `clock_gettime` 都需要把纳秒拆成
  sec/nsec；
- `poll_engine::ns_duration_to_ticks`：poll/nanosleep 超时换算；
- `platform::timer::duration_to_ticks`：timer deadline 换算。

BuildStorm 中 rustc/cargo/glibc 会高频调用时间类 syscall，因此这个底层换算会
被放大很多倍。RISC-V 没有原生 u128 除法，`u128_div_rem` 是完全软件实现，指令
成本远高于 u64 路径。

## 选择方案

把常见时间换算改为 u64 快速路径，保留 u128 fallback 防止极端值溢出：

1. `ticks_to_duration`：
   - `hz == 10MHz` 时 `ns = ticks * 100`；
   - `hz == 100MHz` 时 `ns = ticks * 10`；
   - 通用路径用 `sec = ticks / hz`、`rem = ticks % hz`，再用 u64 计算
     `rem * 1e9 / hz`；
   - 只有 `rem * 1e9` 溢出时才退回 u128。
2. `duration_to_ticks`：把 `Duration::as_nanos()` 先降为 u64，再用 u64 乘加；
   溢出时回退旧逻辑。
3. `ns_to_timespec`：先 `u64::try_from(ns)`，成功则用 u64 除法/取模；
   失败才走 u128。
4. `ns_duration_to_ticks`：先降为 u64，避免 u128 加法与除法。

语义不变：换算仍向下取整/向上取整，保持 deadline 不会提前触发；只减少软件
除法实现的开销。

## 为什么这么做

这是“优化共性调用链”而不是替换基础符号：u128 软件除法是从多个时间 syscall
汇聚到同一个底层实现的共性问题。改动范围集中在平台时间与 syscall 时间路径，
不涉及 allocator、页表、文件系统或调度语义，风险低，适合先做完整 A/B。

## 接下来怎么做

1. 在 `perf/u128-time-conversion` 分支实现上述四个路径。
2. 双架构 `make check`，检查 RISC-V/LoongArch Final。
3. 用完整 RISC-V BuildStorm 对照 main 约 `809.4s` 中位数。
4. 若有效则记录 pc-hot 前后 `u128_div_rem` 和完整耗时；若退化则回退并记录。

## 验证结果：回退

实现后双架构 Final check 通过，短 smoke 通过 toolchain/minibuild 并进入正式编译。
完整 RISC-V BuildStorm 两轮：

```text
u128-time-full-a1: elapsed_s=848.96
u128-time-full-a2: elapsed_s=839.53
```

300 秒 pc-hot 中 `u128_div_rem` 已不再进入前 120，说明这次底层换算改动确实生效；
但完整轮仍明显高于 main 中位数约 `809.4s`，因此不保留。

同时，方向审查结论是不应把 compiler-builtin 的 u128 除法本身当作主要优化对象。
即使它在一段 pc-hot 中占比较高，也应优先从内核调用链上减少或合并时间类 syscall，
而不是替换基础数值实现。本实验已回退并记录，后续不再沿“优化 u128_div_rem 本身”
的方向推进。
