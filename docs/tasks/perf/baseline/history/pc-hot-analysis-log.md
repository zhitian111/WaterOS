# pc-hot 分析记录

每次 pc-hot 采样完成后，把采样数据、对比结论和后续决策记录在这里。原始 `pcs.txt`
保留在 `/tmp`，本文件只保存摘要与可复核路径。

## 2026-08-07 K-32 基线（K-31 commit `bbffe256`）

运行：

```text
run_id: pc-hot-k32-baseline-20260807
date: 2026-08-07 00:31 CST
kernel_commit: bbffe256 (K-31)
workload: Final 早期 180s，cagent + buildstorm toolchain
qemu: qemu-system-riscv64, 8 vCPU, 8 GiB, fresh qcow2 overlay
pcs_path: /tmp/pcs-rv-k32-baseline-20260807.txt
pcs_sha256: 4919388539c55c3464eb3cac17564bcc0bfce38c7d11d7771719d033fc2111c8
raw_log: /tmp/pc-hot-k32-baseline-20260807.log
```

Top-25 摘要：

| 排名 | 计数 | 符号 |
|---|---:|---|
| 1 | 3.56B | `compiler_builtins::memcpy` |
| 2 | 3.07B | `memcmp` |
| 3 | 1.83B | `memset` |
| 4 | 1.28B | `??` |
| 5 | 0.86B | VirtQueue `add_notify_wait_pop` |
| 6 | 0.54B | TLSF `allocate` |
| 7 | 0.49B | `handle_user_page_fault` |
| 8 | 0.44B | block cache `read_blocks` |
| 9 | 0.38B | TLSF `deallocate` |
| 10 | 0.38B | `PagedFileHandle::current_size` |
| 11 | 0.31B | page cache `purge_closed_file` |
| 16 | 0.18B | block cache `cache_put` |

分析：

- block cache `read_blocks` + `cache_put` 仍在内核 Top-20。
- 连续 miss 区间已经先扫描索引，再逐块调用 `cache_put`，会重复一次 `LbaIndex::get`。
- 决策：实现 `cache_put_new`，让 miss 区间直接插入，跳过第二次索引查找。

## 2026-08-07 K-32 当前（工作树 `cache_put_new`）

运行：

```text
run_id: pc-hot-k32-current-20260807
date: 2026-08-07 00:35 CST
kernel_commit: bbffe256 + K-32 working tree
workload: Final 早期 180s，cagent + buildstorm toolchain
qemu: qemu-system-riscv64, 8 vCPU, 8 GiB, fresh qcow2 overlay
pcs_path: /tmp/pcs-rv-k32-current-20260807.txt
pcs_sha256: 436c117f5d7289821f887e3e6983f3d75d6b57619e852a49ccd9d12a3f966336
raw_log: /tmp/pc-hot-k32-current-20260807.log
```

与基线同窗口对比（block-cache 相关指令）：

| 指标 | K-32 基线 | K-32 当前 |
|---|---:|---:|
| 总指令 | 18.14B | 17.32B |
| `read_blocks` | 440.78M | 535.31M |
| `cache_put` | 184.89M | 被内联 |
| `cache_put_new` | - | 被内联 |
| `touch_lru` | 43.28M | 39.46M |
| block-cache 合计 | 668.95M | 574.77M |

分析：

- `cache_put_new` 被 Rust 编译器内联进 `read_blocks`，所以当前 `read_blocks` 单独计数
  上升；按 block-cache 函数合计，当前比基线少约 14%。
- 当前仍保留该优化，进入完整 Final 和 Pre smoke 验收。

后续结果：

- 完整 Final 通过：`elapsed_s=1957.45`。
- 完整墙钟未改善，仍约 1957s；说明当前剩余瓶颈在 block-cache 之外。
- 决策：保留 K-32，因为 block-cache 热路径下降约 14%，且完整 Final/Pre 均通过；
  后续优化应转向 TLSF、VirtIO 和 MM/页缓存路径。

## 2026-08-07 K-30 基线复测（未完成）

为了判断 K-31/K-32 是否拖慢整轮，使用 K-30 commit `3727c056` 重新跑完整 Final。
`axbuild` 已输出 `done (1822.66s)`，但 7 分钟后仍未打印 `BUILDSTORM_COMPILE`，
命中 K-01 已记录的 `cargo xtask` 返回竞态，随后终止本轮。

```text
run_id: k30-full-rerun-20260807
date: 2026-08-07 01:17 CST
kernel_commit: 3727c056
result: inconclusive (known post-build return race)
raw_log: /tmp/k30-full-rv-20260807.log
```

分析：

- 本轮不能证明 K-31/K-32 造成完整轮回退。
- K-32 当前内核在同宿主可完整跑通，且已提交。

## 2026-08-07 K-33 基线（K-32 commit `8fdb047a`）

运行：

```text
run_id: pc-hot-k33-baseline-20260807
date: 2026-08-07 02:00 CST
kernel_commit: 8fdb047a (K-32)
workload: Final 早期 180s，cagent + buildstorm toolchain
qemu: qemu-system-riscv64, 8 vCPU, 8 GiB, fresh qcow2 overlay
pcs_path: /tmp/pcs-rv-k33-baseline-20260807.txt
pcs_sha256: 3a4d2f178eb192691cf07ed1835821a1604334e781e1ab324612667ffd4edaf3
raw_log: /tmp/pc-hot-k33-baseline-20260807.log
```

关键热点：

- `PagedFileHandle::current_size`：331.45M
- `metadata_node`：9.65M
- page cache `purge_closed_file`：305.41M

分析：

- 每次 read/seek/write 都调 `current_size`，其中再逐次锁 ext4 `metadata_node`。
- 决策：读路径改为依赖页缓存逻辑大小 + 句柄打开/截断时记录的磁盘大小，不再逐次
  查询 ext4 metadata。

## 2026-08-07 K-33 当前（工作树 current_size 优化）

```text
run_id: pc-hot-k33-current-20260807
date: 2026-08-07 02:03 CST
kernel_commit: 8fdb047a + K-33 working tree
workload: Final 早期 180s，cagent + buildstorm toolchain
pcs_path: /tmp/pcs-rv-k33-current-20260807.txt
pcs_sha256: 06cb720b358708dc9a4c9aad4b967d5da5ca72242a2d57ea0459ef0bb4c6bbb0
raw_log: /tmp/pc-hot-k33-current-20260807.log
```

关键对比：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.25B | 17.03B |
| `PagedFileHandle::current_size` | 331.45M | 4.12M |
| `metadata_node` | 9.65M | 0.67M |
| `logical_size` | 23.51M | 24.25M |

分析：

- `current_size` 热路径下降约 99%，ext4 metadata 锁基本退出读路径。
- 决策：进入完整 Final 和 Pre smoke 验收；若语义回归则回退。

## 2026-08-07 K-34 页缓存反向索引（已回退）

运行：

```text
run_id: pc-hot-k34-baseline-20260807
date: 2026-08-07 02:43 CST
kernel_commit: 5587cb76 (K-33)
pcs_path: /tmp/pcs-rv-k34-baseline-20260807.txt
pcs_sha256: b243f730817ff91e5a5f1d5fa8546cf033942e8e06f5b24b32b9d21d91ee9de3
raw_log: /tmp/pc-hot-k34-baseline-20260807.log

run_id: pc-hot-k34-current-20260807
kernel_commit: 5587cb76 + K-34 working tree
pcs_path: /tmp/pcs-rv-k34-current-20260807.txt
pcs_sha256: a33d511e0503dba35026aaa9751402cb1f4bb04ff7db33365606151c6963640a
raw_log: /tmp/pc-hot-k34-current-20260807.log
```

关键对比：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.27B | 17.06B |
| `purge_closed_file` | 350.89M | 351.09M |

分析：

- 反向索引去掉了全表扫描（原 `from_iter` 约 11.9M），但新增 `BTreeMap<FileCacheKey,
  BTreeSet<u64>>` 的插入/删除维护成本基本抵消，`purge_closed_file` 无净收益。
- 随后改用 `BTreeMap::range` 只遍历目标文件页键，避免全表扫描且不引入反向索引，
  但 `purge_closed_file` 仍约 `351.16M`，没有净收益。
- 决策：回退 K-34 两个版本，不进入完整 Final。

```text
run_id: pc-hot-k34b-current-20260807
pcs_path: /tmp/pcs-rv-k34b-current-20260807.txt
pcs_sha256: 1677b13aaaf5581fd0ce9d59ad7f8f8f55daa5c5a6931db5e0429cf84834b0e5
raw_log: /tmp/pc-hot-k34b-current-20260807.log
```

## 2026-08-07 K-35 页缓存 FileCacheKey 复用

基线为 K-33 commit `5587cb76`，当前为工作树 K-35。同一 180 秒 Final 早期阶段：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.25B | 17.10B |
| `file_key` | 11.51M | 3.53M |
| TLSF `allocate` | 508.97M | 466.59M |
| TLSF `deallocate` | 365.99M | 336.60M |
| `purge_closed_file` | 350.89M | 350.08M |

```text
run_id: pc-hot-k35-current-20260807
date: 2026-08-07 02:57 CST
pcs_path: /tmp/pcs-rv-k35-current-20260807.txt
pcs_sha256: 863c56eb9e68a2444d7775804d77d94dc76be68e7122b533cfcd376c55311fa6
raw_log: /tmp/pc-hot-k35-current-20260807.log
```

分析：

- read/write/install 路径现在复用同一个 `FileCacheKey`，避免每页重复 `Arc::from`。
- `file_key` 下降约 69%，TLSF allocate/deallocate 各下降约 8%。
- 决策：进入完整 Final 和 Pre smoke 验收。

## 2026-08-07 K-36 关闭句柄不立即 purge 页缓存

基线为 K-35 commit `f3bf2006`，当前为工作树 K-36。同一 180 秒 Final 早期阶段：

| 符号 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 17.10B | 16.49B |
| `purge_closed_file` | 350.08M | 0.03M |
| `file_key` | 3.53M | 3.53M |

```text
run_id: pc-hot-k36-current-20260807
date: 2026-08-07 03:37 CST
pcs_path: /tmp/pcs-rv-k36-current-20260807.txt
pcs_sha256: d9913a13bc5a2cb4e694d3a37a924c3b1c8a5ccb1225b65b85f5dbf7b2ca380f
raw_log: /tmp/pc-hot-k36-current-20260807.log
```

分析：

- 最后 close 只移除 open_refs/files 元数据，缓存页继续由 LRU 保留；unlink/rename
  仍强制 purge。
- `purge_closed_file` 基本退出早期 Final 热点，总指令下降约 3.6%。
- 决策：进入完整 Final 和 Pre smoke 验收。

## 2026-08-07 QEMU `thread=multi` 短采样

显式加入 `-accel tcg,thread=multi` 跑 60 秒 Final 早期，QEMU 进程约 `171%` CPU、
12 个线程，未观察到比默认参数明显更高的宿主核利用率。该方向不改变内核，当前不作为
独立优化提交。

## 2026-08-07 P-core / E-core 亲和性对比

本机 `lscpu -e` 显示 CPU 0-15 为 P-core（最高 5.4-5.6GHz），CPU 16-31 为 E-core
（4GHz）。此前完整测试绑定到 24-31。

| 运行 | CPU 集 | 结果 |
|---|---|---|
| K-36 E-core 完整 | `24-31` | `elapsed_s=1881.13` |
| K-36 P-core 完整 | `0,2,4,6,8,10,12,14` | `elapsed_s=1348.86` |

分析：

- P-core 完整轮比 E-core 快约 28%，`elapsed_s=1348.86`。
- 决策：`rv_final_run.sh` 和 `la_final_run.sh` 默认绑定到 P-core，保留
  `WOS_TASKSET_CPUS` 覆盖。

P-core 上再显式加 `-accel tcg,thread=multi` 做 60s 短采样，进度与不加参数一致
（均到 `BUILDSTORM_MINIBUILD ok`），因此不额外加入启动参数。

## 2026-08-07 K-38 页缓存扩容 16MiB -> 32MiB

`FILE_PAGE_CACHE_CAPACITY` 从 `4096` 调到 `8192`。P-core 完整 Final：

| 配置 | `elapsed_s` |
|---|---:|
| K-36 16MiB 页缓存 | 1348.86 |
| K-38 32MiB 页缓存 | 1282.12 |

分析：

- 32MiB 页缓存完整轮比 16MiB 快约 5%。
- 决策：保留扩容，进入 Pre smoke 验收并提交。

## 2026-08-07 K-39 页缓存 48MiB 实验（已回退）

`FILE_PAGE_CACHE_CAPACITY` 试调为 `12288`（48MiB）。完整 Final 在 cagent 结束后约
8 分钟无串口输出，QEMU CPU 约 724%，疑似页缓存/内核堆压力或进程生命周期停滞。
终止本轮并回退到 32MiB。

```text
run_id: k39-full-pcore-48mib
date: 2026-08-07
result: inconclusive (hang after cagent), reverted
raw_log: /tmp/k39-full-pcore-rv-20260807.log
```

## 2026-08-07 K-41 顺序读预取 16 页实验（已回退）

`FILE_READ_AHEAD_STRIDE` 试调为 `16`（64KiB）。P-core 完整 Final：

| 配置 | `elapsed_s` |
|---|---:|
| K-38 预取 8 页 | 1282.12 |
| K-41 预取 16 页 | 1321.26 |

分析：增大预取反而更慢，已回退到 8 页。

## 2026-08-07 K-42 `-cpu max` 短采样

P-core 60s 短采样使用 `-cpu max`，OpenSBI 报 Base ISA 含 `v`，但 39s 时只到
`BUILDSTORM_TOOLCHAIN ok`；默认 CPU 同窗口已到 `BUILDSTORM_MINIBUILD ok`。
V 扩展模拟开销未带来收益，不加入 Final 启动参数。

## 2026-08-07 K-43 页缓存 8 路组相联索引

`GlobalCacheState.index` 从 `BTreeMap<(FileCacheKey,u64),usize>` 改为 8 路组相联
哈希索引。P-core 180s 同窗口：

| 符号 | K-38 基线 | K-43 当前 |
|---|---:|---:|
| 总指令 | 23.35B | 23.35B |
| `memcmp` | 2.70B | 1.34B |
| 页缓存 BTreeMap insert/remove | 35.83M | 消失 |
| `PageIndex::bucket/get/insert` | - | 0.21B |

分析：

- `memcmp` 下降约 50%，BTreeMap 页索引查找热路径被替换。
- 决策：进入完整 Final 和 Pre smoke 验收。

后续结果：

- 完整 Final 在 `BUILDSTORM_MINIBUILD fail` 停止，说明 8 路组相联索引在 bucket 满
  替换旧条目时没有正确回收到被替换 frame，导致缓存正确性回归。
- 决策：回退 K-43，不进入 Pre smoke，也不提交。

```text
run_id: k43-full-pcore-hash-index
date: 2026-08-07
result: FAILED at MINIBUILD, reverted
raw_log: /tmp/k43-full-pcore-rv-20260807.log
```

## 2026-08-07 K-44 页缓存动态哈希桶（已回退）

用每桶动态 `Vec` 替代固定 8 路组相联，避免 bucket 满替换丢 frame。P-core 完整 Final：

| 配置 | `elapsed_s` |
|---|---:|
| K-38 BTreeMap + 32MiB | 1282.12 |
| K-44 动态哈希桶 | 1292.96 |

分析：

- 动态哈希桶完整 Final 未超过 K-38，且增加内存与代码复杂度。
- 决策：回退 K-44，不进入 Pre smoke，也不提交。

## 2026-08-07 K-45 RISC-V virtio-pci 实验（已回退）

给 RISC-V 增加 PCIe ECAM `0x3000_0000` 的 virtio-blk-pci probe，并把内核 MMIO
恒等映射扩展到 `0x1000_0000..0x8000_0000`。PCI 可正常挂载根卷并进入 MINIBUILD。

完整 Final：

| 配置 | `elapsed_s` |
|---|---:|
| K-38 MMIO | 1282.12 |
| K-45 PCI | 1294.06 |

分析：

- PCI 完整 Final 未超过 MMIO，且需要扩展 1.75GB MMIO 映射与新增平台 probe。
- 决策：回退 K-45，不进入 Pre smoke，也不提交。

## 2026-08-07 K-46 TCG `tb-size=4096` 短采样

P-core 60s 短采样使用 `-accel tcg,tb-size=4096`，进度与默认 TCG 一致
（均到 `BUILDSTORM_MINIBUILD ok`），未观察到明显收益。不加入 Final 启动参数。

## 2026-08-07 K-47 `-cpu rva22s64` 短采样

P-core 60s 短采样使用 `-cpu rva22s64`，进度与默认 CPU 基本一致，约 42s 进入
`BUILDSTORM_BEGIN`。未观察到足以投入完整 Final 的收益，不加入启动参数。

## 2026-08-07 K-48 P-core HT 0-7 完整对比

把 8 vCPU 绑到 `0-7`（4 个物理 P-core 的 8 个线程）跑完整 Final：

| 配置 | `elapsed_s` |
|---|---:|
| `0,2,4,6,8,10,12,14` | 1282.12 |
| `0-7` | 1603.94 |

分析：HT 共享物理核反而明显更慢，现有每物理核单线程配置保持最优。

## 2026-08-07 K-49 QEMU `cache=unsafe` 完整轮

`-drive cache=unsafe` 完整轮构建本体 `done (1267.20s)`，与默认接近；构建完成后约
3 分钟未打印 `BUILDSTORM_COMPILE`，命中已知 `cargo xtask` 返回竞态。该参数未带来
可见收益，不加入 Final 启动参数。

## 2026-08-07 K-50 procfs range 读取

给 `ProcFsView` 增加 `read_range`，FsBridge 的 `/proc` 路径改走 range 接口；静态
proc 文件直接切片，不再整文件分配。完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1281.26 cores=8 bytes=1681000 arch=riscv64
```

分析：该改动是新最优 `1281.26s`，同时修复已知的 procfs 整文件读取路径。

## 2026-08-07 K-51 procfs 小型动态文件栈上 range（已回退）

为 uptime/meminfo/cpuinfo/cgroups/mounts 增加栈上 `SliceWriter` 直接 range 生成。
完整 Final 构建本体 `done (1259.49s)`，但构建完成后约 3 分钟未打印
`BUILDSTORM_COMPILE`，命中已知 `cargo xtask` 返回竞态。本轮不可验收，已回退。

## 2026-08-07 管道继承复现测试

临时把 Final 命令改为 `(sleep 10 &) | cat; echo PIPE_TEST_DONE`。结果正常打印
`PIPE_TEST_DONE`，说明简单后台子进程继承 stdout 不会复现 `cargo xtask` 卡死。
卡死更可能在 cargo/rustc 的多进程、多 fd、exec/exit 组合路径中。

## 2026-08-07 wateros_debug 自动停滞抓取尝试

安装 `gdb-multiarch` 兼容入口后运行 `wateros_debug.py run rv-final --write-disk`。
40 分钟内未出现 stable=10 停滞，debug/GDB 采样开销使完整编译极慢，未捕获现场。
后续需要更轻量的停滞判断或直接复现更短负载。

## 2026-08-07 快速增量 cargo xtask 复现

离线把 K-51 overlay 的 `/glibc/buildstorm_testcode.sh` 改成不删除 target，并在已构建
产物上重跑 `cargo xtask`。结果 `QUICK_BUILDSTORM_RESULT rc=0 elapsed_s=65.42`，
正常返回。说明该竞态只出现在完整重编译的长负载路径，普通增量流程不复现。

## 2026-08-07 K-52 timer 内核态 Exiting 强制退出（已回退）

在 timer 中断进入内核态时检查 `ProcessState::Exiting` 并调用 `exit_group_current`，
用于覆盖远端线程卡在内核未回用户态的场景。完整 Final 通过但
`elapsed_s=1327.53`，比 K-50 的 `1281.26` 慢，且本轮无法证明修复了偶发竞态。
已回退。
