# K-04 Bring-up 诊断热路径隔离（2026-08-05）

## 问题

BuildStorm 每轮处理约 400 万次用户缺页。原 `bringup_stats` 在每次缺页、futex、退出
和回收事件中更新同一组全局 `AtomicU64`，并每 512 次事件尝试生成周期摘要。8 核下
这既产生原子指令成本，也让所有 CPU 争用相同缓存线；诊断代码默认进入比赛构建，
违反 K-04“诊断默认关闭、热路径只做低扰动 per-CPU 累计”的约束。

## 修改

- 新增默认关闭的顶层 `bringup-stats` feature，并透传到 syscall facade 和实现 crate。
- feature 关闭时保留原函数边界，但记录和汇总函数内联为空操作。
- feature 开启时使用 `MAX_CPUS` 个 128 字节对齐的 per-CPU 原子计数分片。
- 只在命令结束检查点遍历分片汇总，不再在热路径周期打印。

修改仅位于 syscall 内部诊断设施和 feature 配置，没有修改 task API、task registry、
调度状态机、runqueue 或锁序。

## 构建验证

以下四种组合均通过：

```text
make rv_check
make la_check
cargo check ... --features qemu-riscv64-opensbi,pre,heap-tlsf,bringup-stats
cargo check ... --features qemu-loongarch64-virt,pre,heap-tlsf,bringup-stats
```

30 秒 RISC-V final 聚焦验证中，两种配置均完成 CAgent 10/10 并进入 BuildStorm；关闭
版没有 `bringup-stats` 输出，开启版汇总 `user_pf=13482`，证明 feature 选择和分片
汇总均生效。

## BuildStorm A/B

两轮顺序使用相同的 RISC-V final 镜像、`-snapshot`、8 vCPU、8 GiB 和宿主物理
P-core 集合 `0,2,4,6,8,10,12,14`：

| 配置 | CAgent | BuildStorm | guest 编译时间 | 参考时间分 |
|---|---:|---|---:|---:|
| `bringup-stats` 关闭 | 10/10 | `ok=true` | 1684.69s | 114.9/120 |
| `bringup-stats` 开启 | 10/10 | `ok=true` | 1812.20s | 105.4/120 |

默认关闭减少 127.51 秒，即相对开启版缩短 7.04%，按 `final-2026` 当前 RISC-V
参考基线 1616.09 秒增加 9.5 个 BuildStorm 原始时间分。开启版最终记录
`user_pf=4025488`，且没有 libc/xgtask 错误、panic、死锁或文件系统后端 I/O 错误。

宿主同时有约 10 GiB Java 背景负载并保留已换出的页面；两轮实时采样大部分时间没有
持续 swap I/O，但本结果仍是单轮 A/B，不替代 K-04 要求的三轮中位数。此前尝试并行
运行两个 8 GiB guest 时出现实时换页，已终止且未纳入结果。

## 证据

```text
off_kernel_sha256=f28d45ab8e3591a90f2db2c4a34409e02f578acd776dd91ab4098b08a87c1c57
on_kernel_sha256=8108b1f2f0909db13a667644f19cb77ed394f40ff71647c9efedd9ccdb57c880
off_log_sha256=24c9a49ee635e04341422952f6c3c262542ae367f147365a6c8a2cca3f0fc113
on_log_sha256=eadc6a00031260f9f09dec677fef297e176454f72b596e7ef171cb8dd6def87d
```

原始日志归档在 `os/debug-reports/archive/k04-bringup-stats-20260805/`，不进入 Git。
