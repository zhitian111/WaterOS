# Task 02 一轮验收进度

## 状态

当前 singlecore 分支的两项低风险热路径改动已提交；按“风险改动一轮验收”的要求，
在宿主机连续 QEMU 后跑了一次 LA 和一次 RV：

```text
LA 一轮: 565.12
RV 一轮: 608.08
```

对应基线：

```text
LA 基线中位数: 538.64
RV 基线中位数: 562.63
```

## 判断

结果明显差于基线，但宿主机长时间 QEMU 后可能已受内存/swap/tmpfs 压力影响，因此
暂不作为最终结论。需要环境恢复后重新做对照；若仍回退，再逐项拆分定位。

## 宿主恢复后复测

宿主恢复到 swap 未使用、无其它 QEMU、可用内存约 24 GiB 后，使用相同 16 GiB、
`-snapshot` 口径各复测一轮：

```text
LA 一轮: 529.46
RV 一轮: 561.14
```

两轮均满足 `TOOLCHAIN_RESULT status=OK`、`MINIBUILD_RESULT status=OK` 和
`BUILDSTORM_RESULT mode=multi status=OK`，编译产物启动验证为 `run=OK`；日志中没有
运行时 panic/OOM/ENOMEM/SIGSEGV。相对基线，LA 快 9.18 秒（约 1.70%），RV 快
1.49 秒（约 0.26%）。因此保留 statx 重复校验去除和 resolver `AtomicPtr` 两项改动，
继续下一阶段热点定位；最终结论仍需双架构各三轮中位数验收。

## 日志裁剪 rebase 后的 allocator 同源对照

本分支 rebase 到本地 `main` 的 `824292aa`（编译期裁剪禁用日志级别）后，分别用同一源码、
同一基础镜像和 `-snapshot` 构建 slab/TLSF 内核。为观察运行顺序影响，LA 先 slab 后 TLSF，
RV 先 TLSF 后 slab：

```text
LA slab:  BUILDSTORM_RESULT elapsed_s=566.05 status=OK run=OK
LA TLSF:  BUILDSTORM_RESULT elapsed_s=538.04 status=OK run=OK
RV TLSF:  内部计时编译 569.78s，随后启动验证停滞，外层 900s 超时
RV slab:  BUILDSTORM_RESULT elapsed_s=580.23 status=OK run=OK
```

LA TLSF 比 slab 快 28.01 秒（约 4.95%），方向和幅度都不支持把 slab 设为 LA 默认。
RV slab 的内部计时编译为 563.77 秒，比 TLSF 的 569.78 秒快约 6.01 秒（约 1.05%），
且只有 slab 完成产物启动验证；但 TLSF 超时发生在计时编译结束后，因此不能把本轮超时
计入编译性能差值，也不能仅凭单轮改变交付配置。仓库 Makefile 与 `make all` 当前仍默认
双架构 TLSF，本轮不修改该稳定默认；slab 继续只作为显式实验配置保留。

以上绝对值均比此前最好单轮偏慢，说明宿主/运行顺序噪声仍然显著；allocator 结论只采用
同批相邻对照的方向，不替代最终三轮中位数验收。
