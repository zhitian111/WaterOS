# RISC-V Rust 编译单核路径优化报告

日期：2026-08-17

## 目标

性能 workload 是 8 vCPU QEMU 内的 BuildStorm Rust 编译。分析表明，`ax-posix-api`、`unwind`、
`hashbrown`、`uefi` 和 `std_detect` 等大 crate 会将匿名页 fault、页表分配和清零集中到执行编译的
繁忙 CPU；因此优化重点是单核执行路径及空闲核协作，而非继续调整编译并行度。

## 已落地优化

1. RISC-V 用户返回路径只恢复一次 `f0..f31` 和 `fcsr`，消除重复的 32 次 `fld`。
2. 全局预清零物理帧池：idle CPU 在 WFI 前批量取得 dirty raw frame，锁外清零并发布；demand miss
   才由当前忙核同步补充。
3. zeroed allocation 接入 lazy fault、零页映射、loader 填充映射，以及 RISC-V/LoongArch 页表页。
4. ELF loader 可在池有超额存量时直接映射完整 pure-BSS 页；无法取得池页时保持 lazy 行为。

详细机制与并发约束见 `2026-08-17-zeroed-frame-pool-design.md`。

## 测量环境

```text
QEMU                 9.2.1
machine              virt
guest memory         16 GiB
guest CPUs           8
disk                 sdcard-rv-pub.img, -snapshot
kernel workload      /glibc/buildstorm_testcode.sh
```

使用的 QEMU：`/home/zhitian/qemu_9_2_1/qemu-9.2.1/build/qemu-system-riscv64`。

## 功能与性能结果

带预清零池和 BSS 优化的已完成 QEMU 运行报告：

```text
BUILDSTORM_RESULT mode=multi status=OK rc=0 cores=8
elapsed_s=540.32
artifact bytes=1681000
run=OK
```

此前 A/B 观察显示，该优化组合相对 baseline 约快 30 秒；组合版本比只启用空白页池约快 10 秒。
这些是多轮环境测量结论，单次运行存在 QEMU/宿主调度噪声，不以本报告中的单一 540.32 秒样本取代。

## 256 页池命中率测量

下表来自独立 reset 的 BuildStorm 统计。该测量内核容量为 256 页、低水位 64 页；统计 feature 仅在
本轮启用，正式配置已关闭。再次进行完整内核统计时需沿 facade feature 链转发该 feature。

| 指标 | 数值 |
| --- | ---: |
| demand hit | 2,158,384 |
| demand miss | 29,864 |
| demand hit rate | 98.6353% |
| demand miss rate | 1.3647% |
| idle refill pages | 1,710,498 (78.16%) |
| synchronous refill pages | 477,824 (21.84%) |
| peak pool length | 256 / 256 |
| final pool length | 224，另有 32 页 in-flight |
| low-watermark activations | 4,135 |
| allocator-lock-busy idle passes | 2,657 |
| OOM drains | 0 |

cagent 前置 workload 的独立统计为 5,629 hit、236 miss，命中率约 96.0%，峰值同样到达 256 页。

## 结论与当前参数

256 页已证明设计有效：绝大多数 demand 页可由零页池直接提供，且超过 78% 的预清零工作由 idle CPU
完成。但是池多次达到容量上限，水位多次重新激活，并保留 1.36% 前台 miss，说明它不能稳定吸收
编译中的 page-fault 突发。

正式参数已调整为 1024 页容量、256 页低水位，即最多缓存 4 MiB 的可回收零页。相对于 16 GiB guest
内存，该上限很小；下一轮使用无探针内核的 A/B 应验证 miss 是否显著收敛及端到端 wall-clock 是否继续
改善。BSS 预映射统计在本轮为零，说明该 workload 未走到可直接消费池页的 pure-BSS 分支，需另行以
包含大 pure-BSS 段的 ELF 定向验证。

原始 QEMU 日志：`/tmp/wateros-rv-zeroed-pool-stats-full.log`。
