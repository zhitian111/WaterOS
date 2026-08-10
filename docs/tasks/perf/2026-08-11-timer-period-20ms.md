# 调度定时器周期 10ms → 20ms 方案（2026-08-11）

## 为什么选择这里

当前 `SCHED_TIMER_PERIOD_MS = 10`。BuildStorm 在 8 个 vCPU 上持续运行约 15
分钟，每个 vCPU 每 10ms 都会进入一次 timer trap，执行 timer re-arm、CPU 记账、
信号计时和调度检查。即使单次成本不高，总量也会随运行时间线性放大。

Linux 的 CFS tick 不再是固定每毫秒必须抢占；WaterOS 当前时间片为约 100ms，
10ms 的 tick 主要用于 CPU 记账、信号计时和调度检查，不是抢占粒度本身。

## 选择的方案

把 `SCHED_TIMER_PERIOD_MS` 从 `10` 改为 `20`。

- 保持 `MAX_TICKS_PER_TASK` 等时间片语义不变。
- 每 vCPU 的 timer trap 频率减半。
- `times/getrusage` 等 tick 计数仍按 tick 累计，用户可见语义不变。

## 为什么这么做

1. 这是纯配置改动，不触碰调度算法、runqueue、负载均衡或信号状态机。
2. BuildStorm 是长时重负载，固定周期 trap 的开销值得先用 A/B 验证。
3. 若 20ms 没有收益，回退成本为零。

## 接下来的工作

1. 在 `perf/timer-period-20ms` 分支修改配置。
2. 双架构 Final check 与 180 秒 smoke。
3. RISC-V 完整 BuildStorm A/B；相对当前 main 有 ≥ 1.5% 净改善才合并。
4. 完成后补 wait-hot/pc-hot 分析并归档。

## 验收标准

- 双架构 Final check 通过。
- 调度、定时器、`times/getrusage`、信号计时无回归。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
timer-20ms-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=816.12
main-cow-full-b1:   BUILDSTORM_COMPILE ok=true elapsed_s=817.27
```

完整 BuildStorm 成功，无 panic/SIGSEGV，但只快约 `1.15s`（0.1%），未达到合并
门槛。实现已回退，仅保留本记录。
