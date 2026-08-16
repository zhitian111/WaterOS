# Slab 与单核编译链路审计任务

本目录对应分支 `perf/slab-singlecore-audit`，工作树为
`/tmp/wateros-singlecore-compile-vfs`。目标是修复当前 frame-backed slab
相对纯 TLSF 全局堆的退化，并在不牺牲功能语义的前提下优化 BuildStorm 中由少数
大型 crate 拖慢的串行编译链路。

## 总原则

- 每个任务文档对应一个独立提交；提交前必须完成该文档的静态/功能门禁和最窄性能 A/B。
- 每个任务完成后新增 `history/<task-id>-brief.md`，记录改动、验证命令、结果、未验证项和
  是否满足进入下一任务的门槛。
- 功能正确性是硬门禁；性能实验可以暂时退化，但最终 RV 与 LA 都必须优于本任务保存的
  `main` 基线。
- 不修改 `~/Downloads/*.img.gz` 原始镜像，不把性能镜像、内核 ELF、日志和 `target/`
  加入提交。
- CodeGraph 已初始化；源码定位优先使用 `codegraph explore`、`codegraph callers`、
  `codegraph impact`，再用 `rg` 补充。

## 基线与外部输入

| 项目 | 路径/值 |
|---|---|
| main 基线分支 | `main`，当前基线提交以任务 00 实际记录为准 |
| LA 原始镜像 | `~/Downloads/sdcard-la-pub.img.gz` |
| RV 原始镜像 | `~/Downloads/sdcard-rv-pub.img.gz` |
| BuildStorm 脚本 | `~/Downloads/buildstorm_testcode.recovered.sh` |
| 镜像内目标 | `/glibc/buildstorm_testcode.sh` |
| QEMU 根目录 | `~/qemu_9_2_1/qemu-9.2.1/build` |

每轮测试必须：从 gzip 原始镜像重新解压到新 raw 文件，使用 `debugfs` 删除并写入恢复脚本，
设置脚本为 `0755`，然后使用 `-snapshot` 启动。禁止复用已经运行过的 raw 镜像。

## 任务顺序

| 任务 | 独立提交目标 |
|---|---|
| `00-baseline-and-harness.md` | 固化 main 内核、镜像 SHA、QEMU 和运行脚本 |
| `01-remove-global-slab-counters.md` | 从默认热路径移除全局 slab 统计原子操作 |
| `02-route-and-guard-overhead.md` | 消除重复 size-class 路由和非 slab fallback 的额外 guard |
| `03-low-overhead-slab-diagnostics.md` | 增加 per-CPU、feature-gated 诊断，量化 remote/refill/page 状态 |
| `04-slab-page-reclaim.md` | 安全回收空页并修正 slab/frame 内存统计 |
| `05-remote-free-rebalance.md` | 有界 remote-free、按 class 分流和 owner/页再平衡 |
| `06-singlecore-allocator-ab.md` | `smp=1` 与正常 SMP 的 allocator 归因实验和门禁 |
| `07-buildstorm-crate-attribution.md` | 为大型 crate 记录开始/完成时间和内核链路画像 |
| `08-selected-serial-chain-optimization.md` | 依据任务 07 数据实施一个可解释的串行链路优化 |
| `09-final-functional-acceptance.md` | 双架构功能、压力、镜像一致性终验 |
| `10-final-performance-acceptance.md` | main 与最终候选的双架构三轮中位数验收 |

任务 08 的具体调用链必须由任务 07 的数据选出；若证据不足，提交诊断结论并停止扩大改动，
不得预先假定 dcache、readahead 或 exec prefix 一定有效。

