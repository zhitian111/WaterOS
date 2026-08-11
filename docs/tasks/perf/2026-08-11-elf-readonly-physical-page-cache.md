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

`from_elf_path` 对 resolved path 只打开一次只读 `VfsIoHandle`，ELF header、program
headers 和后续 segment fault 都使用该 handle 的 `read_at`。segment loader 和 fork 副本
共享 handle 所有权，避免每个 4 KiB fault 重新做绝对路径规范化和逐组件 ext4 lookup。

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

待完成后补充。
