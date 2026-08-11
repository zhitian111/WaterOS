# 跨进程共享只读 ELF 物理页

## 为什么选择这里

独立画像分支 `perf/elf-page-share-profile`（提交 `b5f9d744`）在固定 BuildStorm 300 秒
窗口得到：

| 指标 | 数值 |
| --- | ---: |
| 可共享只读 ELF lazy faults | 40,960 |
| 唯一内容页键 | 7,325 |
| 重复装载 | 33,635 |
| 重复率 | 82.11% |
| 可避免清零/复制下界 | 131.39 MiB |
| 唯一只读工作集 | 28.61 MiB |

该结果超过预设的 30% 重复率和 128 MiB 可避免复制门槛。当前每次
`ElfPathSegmentLoader` fault 都重新分配、清零物理帧，再从 VFS page cache/文件系统复制
4 KiB。Linux 会让多个进程的干净只读文件页指向 page cache 中的同一物理页；WaterOS
已经有帧引用计数，但尚未把它用于 exec ELF 页。

这项优化覆盖反复启动的 cargo、rustc、linker、动态解释器和 libc，不针对某个符号；画像
显示理论命中率足以产生明显的端到端收益。

## 选择的方案

### 1. 持久只读 VFS handle

每个 lazy ELF segment 在注册时打开一个持久只读 `VfsIoHandle`，后续 segment fault 都使用
该 handle 的 `read_at`；fork 产生的 loader 副本共享 handle 所有权。ELF header/program
header 的现有读取路径保持不变。这样避免每个 4 KiB fault 重新打开路径，同时避免不同
segment 共用一个 handle 锁而扩大竞争。

### 2. 稳定文件身份与内容代次

VFS 为稳定文件 handle 暴露：

- mount generation；
- mount id + stable node/inode id；
- 一个跨同 inode handle 共享的原子 content version token。

任何成功的 write/pwrite/truncate 和 unlink/replace 都推进 content version。ELF cache key
包含 version，因此文件修改后旧物理页只能作为已有进程的旧映射继续存活，不会命中新 exec。
缓存条目持有 token 的 `Arc`，保证句柄全部关闭后重新打开同一 inode 仍获得同一代次对象；
unlink 会先推进旧 token，避免 inode 复用与旧 cache key 冲突。

### 3. 有界只读 ELF PPN cache

在双架构共享的 mm impl-common 中建立 16,384 页（64 MiB 上限）的缓存。键包含文件身份、
content version、ELF segment 布局与 loader file offset；值为 PPN 和 LRU tick。

- miss：分配/清零/读取一次；初始 frame ref 归缓存所有，再为即将建立的映射加一个 ref；
- hit：`frame_inc_ref` 后把同一 PPN 映射到另一个地址空间；
- race miss：允许锁外并行读取，发布时若已有赢家则释放重复帧并使用赢家；
- eviction：移除最旧 cache ref 并 `frame_dealloc`，现存进程映射的引用继续保证物理页存活；
- 只允许 `!PagePerm::W` 的 ELF segment 使用 cache；可写 data/BSS 仍走独立帧。

### 4. MM loader 合约

给 `DemandPageLoader` 增加可选 shared-page 方法。默认 loader 返回 `None`，保持匿名 mmap、
普通 lazy file mmap 和既有实现不变；ELF loader 返回一个已持有“映射引用”的 PPN。页表映射
失败时 caller 负责释放该引用，地址空间正常销毁继续沿用现有 `dealloc_frame`。

## 为什么这样做

- PPN 共享同时消除帧分配、整页清零、VFS-to-frame copy 和重复路径解析，而不只是把读取
  从设备提升到内存；
- 只读 PTE 不需要 COW，语义比共享可写映射简单；
- 内容代次使优化不是 BuildStorm 特判：工具链或普通 ELF 被覆盖后不会执行陈旧代码；
- 64 MiB 上限覆盖画像中约 28.6 MiB 的 300 秒工作集，同时不会无界吞噬物理内存；
- 缓存/映射分离引用允许安全 eviction，不需要扫描所有地址空间。

## 接下来的优化工作

1. 扩展 VFS handle 内容身份契约，在 fs-bridge 的稳定 node registry 中维护共享 version
   token，并覆盖 write/pwrite/truncate/unlink 变化路径。
2. 在 mm impl-common 实现有界 PPN cache、竞争 miss 发布和引用计数。
3. 扩展 DemandPageLoader，双架构 lazy fault 优先请求 shared page。
4. 双架构 ELF loader 改持久 handle；只读 segment 接入 cache，可写 segment 保持旧路径。
5. 定向测试：同键只装载一次/同 PPN、版本变化 miss/引用计数、可写页不共享、映射失败释放。
6. 运行相关 crate tests、`rv_check`、`la_check`、RISC-V Final build 和 BuildStorm smoke。
7. 运行 matched pc-hot，关注 `handle_user_page_fault`、frame allocator、memset/memcpy、
   normalize/lookup 是否下降。
8. 固定镜像、CPU `0-15`、`TMPDIR=/tmp`、`-snapshot` 进行 candidate/main/candidate 完整
   交错 A/B。

## 验收与回退门槛

- 文件修改后新 loader 必须 miss；只读共享页在任一进程退出后仍可被其他映射安全访问；
- 双架构 check/build 与定向测试全部通过，BuildStorm 无权限、ELF、引用计数或 OOM 回归；
- 完整候选至少两轮，相邻 main 对照下改善可复现且至少 1.5%；本项以 10% 级收益为目标；
- 若持久 handle 的全局锁或 PPN cache BTree/LRU 成为新热点，拆分或回退；
- 未达到门槛时不合入 main，只在本分支保留文档与实验提交。

## 实现与结果

### 实现

- VFS handle 新增稳定的 `(mount generation, mount id, node id, content version)` 内容身份；
  fs-bridge 对同一稳定节点共享 version token，并在 write/pwrite/truncate/unlink 路径推进版本。
- `DemandPageLoader` 新增默认返回 `None` 的可选 shared-page 合约；普通 mmap/匿名 loader 不变。
- RISC-V 与 LoongArch ELF loader 对只读 segment 使用共享 PPN，可写 data/BSS 保持独立帧。
- `mm-impl/common` 增加 16,384 项有界缓存；miss I/O 在锁外完成，竞争发布只保留赢家，
  eviction 只释放 cache ref，不影响已存在映射。
- 定向 MM 自检覆盖同键只加载一次、相同 PPN、cache/mapping 引用计数以及内容版本变化 miss。

### 正确性与构建验证

- RISC-V Final check：通过。
- LoongArch Final check：通过。
- RISC-V Final kernel build：通过，候选内核 SHA-256：
  `4e09fc3f2dadc3af45fc0d7ba7cb74bbf8db9b9eac5b0b97f554dac6f6bdd86e`。
- 180 秒 smoke：通过 toolchain/minibuild，进入正式编译；无 panic、SIGSEGV 或 stall。
- 两轮完整候选均通过 toolchain、minibuild、compile marker 和 judge。

### 完整 BuildStorm A/B

全部样本使用固定镜像
`4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1`、CPU `0-15`、
`TMPDIR=/tmp` 与 QEMU `-snapshot`。main 三个样本的内核 SHA-256 均为
`ea091ca43109ad0ae13b2da3ce14acb2504d60cc1e8ea8f0a6b49441450082d2`。

| 实现 | elapsed_s |
| --- | ---: |
| main | 810.26 |
| main | 803.36 |
| candidate | 793.85 |
| main（相邻对照） | 811.03 |
| candidate | 806.49 |

- main 中位数：`810.26s`；candidate 中位数：`800.17s`。
- 中位数净减少 `10.09s`，约 `1.25%`；均值口径约改善 `1.0%`。
- 两轮 candidate 都完整通过，且没有出现性能倒退；第一轮相对相邻 main 快 `2.12%`，
  第二轮相对 main 中位数快约 `0.47%`。
- 结果低于原先自动合并门槛 `1.5%`，但用户明确接受该稳定正收益并决定纳入 main。

### 300 秒 pc-hot 核验

main 与 candidate 使用上述固定镜像和对应固定内核；两边均通过 toolchain/minibuild 并进入
正式编译。candidate 在相同宿主窗口内执行 `34.06B` guest 指令，main 为 `32.06B`，进度
指标增加 `6.24%`。归一化热点变化：

| 热点 | main 占比 | candidate 占比 | 相对变化 |
| --- | ---: | ---: | ---: |
| VirtIO `add_notify_wait_pop` | 4.186% | 3.089% | -26.20% |
| user page fault | 1.003% | 0.906% | -9.69% |
| TLSF allocate | 3.558% | 3.481% | -2.16% |
| TLSF deallocate | 2.515% | 2.455% | -2.39% |
| memset | 5.732% | 5.691% | -0.71% |
| compiler memcpy | 11.408% | 11.466% | +0.51% |

新增 ELF cache BTree 查找仅占 candidate 总指令 `0.081%`，`load_or_get` 包装占
`0.019%`，content identity 获取占 `0.008%`，均未形成新热点。VirtIO 与 page-fault
占比下降证明共享页确实减少重复缺页读取；全局 memcpy 基本不变，说明剩余复制主要来自
编译器和普通文件路径，也解释了完整墙钟收益只有约 1%。

### 决策

按用户确认保留并合并。后续不继续在 ELF cache 内做锁或 BTree 微调；优先转向重复
pathname/metadata 的 negative dentry cache，以及 per-CPU 小对象分配快路径。
