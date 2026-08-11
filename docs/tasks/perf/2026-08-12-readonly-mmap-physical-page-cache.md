# 普通只读私有 mmap 跨进程物理页缓存实验

## 为什么选择这里

BuildStorm 会反复启动 cargo、rustc、linker 和辅助进程。current main 已把 exec ELF 的只读
PT_LOAD 页按稳定文件身份跨进程共享；300 秒画像中该缓存命中约 80%，显著减少了 VirtIO wait
和用户缺页，但墙钟收益只有约 1%，原因之一是用户态动态链接器加载 `.so` 时走普通文件
`mmap`，并不使用 exec ELF loader。

当前 `VfsMmapPageLoader` 的每个 fault 都执行：分配物理帧、清零 4 KiB、从 VFS page cache
复制 4 KiB、安装 PTE。相同动态库页在多个 rustc/linker 地址空间里仍重复这条链路。syscall
画像也确认 sysroot、libc、Cargo registry 路径有很高复用率，因此普通只读文件映射是比继续
微调 TLSF 原子操作更有上限的候选。

历史 `feat/file-page-sharing` 曾尝试直接映射 VFS page-cache frame，并为 write/truncate/fork
建设反向映射和跨地址空间失效协议，改动约 1500 行且没有形成可验收结果。本实验不复刻该架构：
只处理初始不可写的 `MAP_PRIVATE` 文件映射，使用 current main 已验证的内容代次和帧引用计数，
把语义范围收缩到不可变快照。

## 方案

在双架构共用的 `mm-impl/common` 新增独立的只读 mmap 物理页缓存：

- key：mount generation、mount id、stable node id、content version、页对齐 file offset，以及
  mmap 建立时捕获的 file size（避免 grow 后旧 loader 的 EOF 快照污染新 mapping）；
- value：物理页号、最近访问 tick，以及持有 version token 的文件身份；
- 容量：16,384 页（64 MiB），和 exec ELF cache 分离，避免普通 mmap 淘汰已证明有效的 ELF
  工作集；
- miss I/O 在 cache lock 外完成；竞争 miss 只发布赢家；cache 与每个 PTE 各持有一个 frame
  ref；容量满后的新 miss 绕过 admission，不执行线性 victim 扫描。

`VfsMmapPageLoader` 在创建时记录
`allow_readonly_sharing = MAP_PRIVATE && perm.executable() && !perm.writable()` 和
稳定内容身份。`load_shared_page` 仅在该条件成立且 identity 可用时查缓存；其余映射沿用原来的
分配/复制路径。

语义边界：

1. 文件 write/truncate/unlink 已推进共享 content version；之后的新 fault 不能命中旧页。
2. 已经映射的 `MAP_PRIVATE` 页可以继续保留旧快照，不要求随文件 write 更新。
3. 后续 `mprotect(W)` 会进入现有 `ensure_private_for_write`；cache 至少持有一个额外 ref，故
   写权限建立前会复制为进程私有页。
4. `MAP_SHARED`、初始可写 private mmap、无稳定 identity 的 handle 完全不共享。

## 为什么接近 Linux 做法

Linux 的 file-backed private mapping 会让多个地址空间的干净页指向 page cache 中同一物理页，
写入时再 COW。WaterOS 现有 VFS page cache 仍是固定字节槽，不能直接映射为 PTE；本候选先用
独立有界 PPN cache兑现跨进程共享，避免为了一个性能实验立即引入完整 address_space、folio、
rmap 和 writeback 协议。若该方向有明显收益，后续再统一 VFS page cache 与可映射 frame。

## 实施与验证

1. 在 `mm-impl/common` 实现普通 readonly mmap cache、版本重检、竞争发布和满容量 bypass。
2. 在 mm 聚合层只转发这一内部服务；不修改 `api-v0` 的 `DemandPageLoader` 合约。
3. 扩展 `VfsMmapPageLoader` 的 duplicate/load_shared_page，并把映射 flags/perm 传入创建路径。
4. 定向自检覆盖同 identity+offset 复用、offset 隔离、version 变化 miss、frame ref 生命周期。
5. 运行 RISC-V check、RV/LA Final `make all`，核对默认别名与脚本正文 marker。
6. 普通 Final 只跑一轮完整 BuildStorm；首次明显改善即接受，持平/退化不做第二轮。
7. 若接受，再用 diagnostics 或 pc-hot 核验 mmap cache 命中以及 page-fault/copy/VirtIO 变化。

## 接受与回退条件

- 所有 marker 通过，无 stale 内容、COW、frame ref、OOM、panic 或 stall 回归；
- 相对 current-best 783.00s 至少给出超过近期约 10--13 秒抖动的明确改善；
- 若 cache 命中低、BTree/LRU 开销抵消收益或只落在噪声内，不合入 main，仅保留实验文档。

## 首轮全只读映射结果与收缩

首轮实现覆盖所有初始不可写的 `MAP_PRIVATE` file mmap。候选通过 RV/LA 构建、全部
BuildStorm marker 和 judge，无 panic、stall 或 SIGSEGV，但编译时间为 932.23s，相对
current-best 783.00s 回退 149.23s（19.06%）。结果文件：
`/tmp/wateros-buildstorm-fixed/readonly-mmap-physical-page-cache-a1/result.json`。

从实现结构可确定两个高风险放大器：普通只读 mmap 同时覆盖大量一次性的 rmeta、archive 和
中间产物，16,384 页容量到顶后，每次新页都线性扫描全部 BTree entry 选择 LRU victim；此外，
被 cache 持有的只读数据页若稍后 `mprotect(W)`，现有私有化逻辑会因额外 frame ref 复制整页。
这两项都不属于共享动态库代码页的目标收益。

修订候选只允许 `MAP_PRIVATE && executable && !writable`，聚焦动态库 text；普通只读数据映射
完全回到原路径。缓存满后不再 O(n) 淘汰，而是保留已建立热点集，让新 miss 仅使用本次已加载
frame、跳过 admission。首轮是明确失败，按规则只再运行这一轮基于根因收缩的候选。

## 可执行页收缩候选结果

修订版同时通过普通 RISC-V check 和 `cache-layer-diagnostics` feature check，随后通过 RV/LA
`make all`。默认别名与 Final 一致且均保留脚本正文 marker：

- RV Final SHA-256：`5c1298412706eeae6bd0e44891b43c08353ab87f3ca317c76402562355ca19f4`；
- LA Final SHA-256：`0496904a2791dbb0cc2b8e145363f7a9aa2bfc777aec02c7d8fc53185f4ac63a`。

同一镜像、runner 和 CPU 条件下，完整 BuildStorm 结果：

| 内核 | elapsed_s | 相对 current-best 783.00s |
| --- | ---: | ---: |
| readonly executable private mmap cache | 640.95 | -142.05s（-18.14%） |

候选通过 toolchain、minibuild、compile marker 和 judge，产物 1,681,000 字节；无 timeout、
stall、panic 或 SIGSEGV。judge 的 compile-time 项得分 47.7。结果文件：
`/tmp/wateros-buildstorm-fixed/readonly-exec-mmap-physical-page-cache-a1/result.json`。

18.14% 明显越过噪声和预设接受线，也与“动态库 text 高复用、普通只读数据接近一次性”的收缩
假设一致。按照首次明确有效即停止的验收规则，不运行第二轮性能样本，接受并合入 main。后续
diagnostics 只用于记录 RX cache 命中/驻留量，不改变本次墙钟结论。

## 300 秒 diagnostics 核验

合入后使用独立 worktree 的 `cache-layer-diagnostics` 内核运行固定 300 秒窗口。该轮按预期在
compile marker 前由 runner timeout，只作画像；toolchain/minibuild 已通过，无 stall、panic
或 SIGSEGV。最后一组累计值：

| 缓存 | lookups | hit / 命中率 | miss | resident / 满容量 bypass |
| --- | ---: | ---: | ---: | ---: |
| readonly executable mmap | 344,064 | 277,389 / 80.62% | 66,675 | 16,384 / 50,123 |
| exec ELF readonly | 32,768 | 25,647 / 78.27% | 7,121 | 6,877 / 0 eviction |

RX mmap cache 的高命中率证明动态库代码页确实被大量跨进程复用；同时 50,123 次 miss 因容量
已满而绕过 admission，说明 64 MiB 并未覆盖完整 RX 工作集。诊断结果文件：
`/tmp/wateros-buildstorm-fixed/readonly-exec-mmap-cache-diag-300s/result.json`。下一项容量实验可
独立验证 128 MiB 是否继续减少缺页/I/O；不能把该诊断轮当作新的墙钟成绩。
