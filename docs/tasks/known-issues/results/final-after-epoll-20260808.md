# 双架构完整 Final 复验（2026-08-08）

## 运行内容

在已提交 `sched_setaffinity EPERM` 与 `epoll_pwait2` 修复后，分别运行
RISC-V64 / LoongArch64 决赛 BuildStorm：

```text
RISC-V: WOS_TASKSET_CPUS=0,2,4,6,8,10,12,14
        make run ARCH=rv PROFILE=final
LoongArch: WOS_TASKSET_CPUS=0,2,4,6,8,10,12,14
        make run ARCH=la PROFILE=final
```

日志：

- RISC-V：`/tmp/final-after-epoll-20260808.log`
- LoongArch：`/tmp/final-after-epoll-la-pcore-20260808.log`

## 结果

RISC-V：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1144.48 cores=8 bytes=1681000 arch=riscv64
```

LoongArch：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1083.76 cores=8 bytes=1714568 arch=loongarch64
```

两架构均无 panic，`cargo xtask` 返回竞态未复现。

## 并行尝试

LoongArch 曾同时绑定 E-core 集合
`16,18,20,22,24,26,28,30` 运行，48 分钟后仍停留在同一批 `Compiling chrono/...`
输出且串口 20 分钟无变化，判断为卡住后终止；随后在 P-core 集合上重跑并通过。
该 E-core 日志保留在 `/tmp/final-after-epoll-la-20260808.log`，不作为有效成绩。
