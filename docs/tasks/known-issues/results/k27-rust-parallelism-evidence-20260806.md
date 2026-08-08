# K-27 Rust `available_parallelism` 测量证据（2026-08-06）

## 目的

确认 BuildStorm 编译阶段 Cargo 实际看到的并行度，避免把“8 vCPU 空闲”误判成
`sched_getaffinity` 或 job 数量问题。

## 验证

在决赛镜像 guest 内使用 Rust 标准库直接输出：

```text
parallel=8
```

命令：

```sh
printf '%s\n' 'fn main(){println!("parallel={}", std::thread::available_parallelism().unwrap());}' > /tmp/par.rs
rustc /tmp/par.rs -o /tmp/par
/tmp/par
```

结论：Cargo/rustc 已经看到 8 个可用 CPU。完整 BuildStorm 统计中约 51% 的 scheduler
tick 处于 idle，主要来自早期 `core/std/compiler_builtins` 的串行依赖阶段和后续
依赖图不能满 8 核并行，而不是 Cargo 并行度被限制。

完整统计日志：

```text
/tmp/k25-full-stats-now-rv-1785999510.log
```

最终统计：

```text
syscalls=943982
user_pf=4025019
ctx=1391845
idle_ticks=624630
timer_ticks=1220517
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1607.53
```
