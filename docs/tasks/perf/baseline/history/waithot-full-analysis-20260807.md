# BuildStorm 性能采样分析报告（2026-08-07）

## 背景与目的

当前 Final 完整 BuildStorm 本地最优为 `elapsed_s=1281.26`，目标是把本地完整执行时间
压到 `700-800s`。本次目标不是直接优化，而是先在干净基线上拿到完整的 pc-hot 指令热点
和每核 wait/idle 数据，为后续优化排序提供依据。

本轮没有修改内核代码。wait-hot 是独立 QEMU 插件，通过 vCPU idle/resume 回调统计 WFI
停留时间，并记录 WFI PC。

## 测试环境

- 架构：RISC-V64 / OpenSBI / QEMU `virt`
- vCPU：8，内存：8 GiB，磁盘：`os/sdcard-rv-pub.img`，使用 `-snapshot`
- 宿主绑定：P-core，`0,2,4,6,8,10,12,14`
- 内核：已提交 K-50 干净基线
- 工具：
  - `pc-hot`：逐 PC 指令计数
  - `wait-hot`：每核 WFI idle 时间与 WFI PC

## 执行结果

| 轮次 | 工具 | 结果 |
|---|---|---|
| 双插件完整 Final | pc-hot + wait-hot | cagent 10/10；BuildStorm 进入编译，28 分钟后命中 `cargo xtask` 返回竞态，无 `BUILDSTORM_COMPILE` |
| wait-hot 完整 Final | wait-hot | cagent 10/10；BuildStorm 完成 `axbuild done (1204.05s)`，随后命中 `cargo xtask` 返回竞态 |
| 干净基线短测 | pc-hot / wait-hot 分开 | cagent 通过，BuildStorm toolchain/minibuild 通过，无 panic |

两轮完整轮都未能得到可验收的结束标记，因此本轮数据不能作为最终性能成绩，但覆盖了
BuildStorm 早期到 axbuild 完成的主要路径。

## pc-hot 指令热点

双插件 28 分钟轮采集到 `281,052,786,769` 条指令。Top 热点如下：

| 排名 | 符号 | 指令数 |
|---|---:|---:|
| 1 | `Sv39AddressSpace::mprotect` | 82.28B |
| 2 | `memcpy` | 57.73B |
| 3 | `??` | 23.79B |
| 4 | `memset` | 13.66B |
| 5 | `handle_page_fault` | 12.81B |
| 6 | `with_scheduler` | 12.32B |
| 7 | `memcmp` | 11.51B |
| 8 | VirtQueue `add_notify_wait_pop` | 7.99B |
| 9 | TLSF `with_allocator_interrupt_guard` | 7.19B |
| 10 | TLSF `allocate` | 4.46B |

分析：

- `mprotect` 单独占 82.28B，远超其它内核符号，是当前最值得先验证的方向。BuildStorm
  或用户态 runtime 会频繁改变页权限，若这是真实热点，说明用户态 `mprotect` 调用链或
  内核页表路径仍很重。
- `memcpy` / `memset` / `memcmp` 合计约 83B，仍是用户态与内核数据搬运的主要开销。
- `handle_page_fault` 与 `with_scheduler` 说明缺页、trap 和调度临界区仍在热路径。
- VirtQueue `add_notify_wait_pop` 与 block cache/TLSF 仍是内核侧 I/O 与堆热点。

## wait-hot 每核 idle 数据

wait-hot 完整轮在 axbuild 完成前的采样结果：

| CPU | WFI idle_ms | WFI PC |
|---|---:|---|
| 0 | 1191600 | `0x8030d4ac` |
| 1 | 827206 | `0x8030d4ac` |
| 2 | 1171588 | `0x8030d4ac` |
| 3 | 1193672 | `0x8030d4ac` |
| 4 | 1204302 | `0x8030d4ac` |
| 5 | 629270 | `0x8030d4ac` |
| 6 | 1098327 | `0x8030d4ac` |
| 7 | 1182251 | `0x8030d4ac` |

`0x8030d4ac` 对应 `__wateros_idle_task_runtime_main`。

分析：

- 各核 idle 时间差异很大。CPU5 约 `629s`，CPU1 约 `827s`，其它核约 `1098-1204s`。
- 这说明 BuildStorm 编译阶段存在明显负载不均，不是所有核都被同时充分利用。
- 若继续只优化单个符号，可能仍会被“一两个忙核 + 多个空转核”的结构限制；调度器负载
  均衡和 jobserver/任务分配是另一个高价值方向。
- 注意：wait-hot 统计的是 vCPU 进入 WFI 的 idle 时间，不是 guest 内部任务阻塞在
  waitqueue 上的等待时间；后者在 QEMU system emulation 下无法仅靠 syscall 回调获得。

## 结论

1. 本轮没有可验收的完整 Final 成绩，阻断点是已知 `cargo xtask` 返回竞态，而不是镜像、
   插件或基线本身。
2. 指令热点最突出的是 `mprotect`、`memcpy/memset/memcmp`、page fault、scheduler、
   VirtIO 与 TLSF。
3. WFI 数据显示 BuildStorm 编译期 CPU 负载不均，CPU1/CPU5 明显更忙，其它 CPU 大量
   时间空转。
4. 在拿到可验收完整轮之前，应优先修复 `cargo xtask` 返回竞态；否则完整 pc-hot/wait-hot
   可复现性很差，性能优化也很难准确验收。

## 下一步建议

- P0：修复 `cargo xtask` 构建完成后的返回竞态，恢复双架构完整 Final 稳定性。
- P1：用完整 pc-hot/wait-hot 复测后，先验证 `mprotect` 是否真实占主导，再决定优化
  用户态调用频率或内核页表路径。
- P1：针对 CPU1/CPU5 更忙、其它核空转的现象，分析调度器负载均衡和 Cargo jobserver
  的任务分布。
- P2：继续降低 TLSF、VirtIO 与用户态内存函数热点，但要等完整轮稳定后再作为验收依据。

## 原始材料

```text
/tmp/waithot-full.log
/tmp/waithot-full-pcs.txt
/tmp/waithot-full-wait.txt
/tmp/waithot-only-full.log
/tmp/waithot-only-full-wait.txt
```

SHA-256：

```text
c7921375a42c81cc5b9d96db9d1fe40964f56673bb0426e8d6aa857ef1953b25  /tmp/waithot-full.log
b3fac0595d0c9b3ea17924b9768c05505d0e000dd6a842835c7b43ec83ebd842  /tmp/waithot-full-pcs.txt
883f8d4af00622846bb496e8b6a47fad8dba0b8bf06468bc5918c240e47a2242  /tmp/waithot-full-wait.txt
49b7bce76cb345bc08e9c62e1e479f81552c82c0064ee946d705b67e1c79142b  /tmp/waithot-only-full.log
81d00e6bc18f877602f61cc3c484577de6b4a7fda0ab417c1aea53b30dff47ee  /tmp/waithot-only-full-wait.txt
```
