# BuildStorm Linux baseline 优化任务与路线图（2026-08-10）

## 目标与判定口径

本轮不以某台机器上的固定秒数作为唯一目标，而使用同宿主、同 QEMU 参数、同镜像下的
Linux/WaterOS 比值：

```text
R = WaterOS BUILDSTORM_COMPILE elapsed_s / Linux BUILDSTORM_COMPILE elapsed_s
```

- 阶段一：`R < 2.0`，并能稳定完整输出 `BUILDSTORM_COMPILE ... ok=true`。
- 最终目标：`R <= 1.0`；达到后再以 `R < 1.0` 为超越 Linux 的优化目标。
- 所有性能结论至少使用 3 次完整轮的中位数；短窗口只用于筛选，不用于验收。
- 所有运行必须使用 `-snapshot`，避免前一轮磁盘写入污染后一轮。

当前同机 Linux 6.12.102 基线：

| 架构 | QEMU 配置 | Linux | 最近 WaterOS 有效结果 | 当前可用结论 |
|---|---|---:|---:|---|
| RISC-V | 16 GiB / 8 vCPU | 395.90s | 1031.52s（8 GiB / 8 vCPU） | `R ~= 2.61`；阶段一等价目标约 791.8s |
| LoongArch | 36 GiB / 12 vCPU | 353.55s | 1106.30s（8 vCPU） | CPU 数不一致，必须先重测 12 vCPU |

因此 RISC-V 阶段一至少需要约 23.2% 的完整轮改善，最终目标需要约 61.6%。
LoongArch 在得到 12 vCPU WaterOS 数据之前，不报告改善百分比。

官方 `final-2026` judge 的参考常数是 RISC-V 1616.09s、LoongArch 1985.21s，但它们
来自另一台宿主。优化决策必须优先看 `R`，不能把本机 WaterOS 秒数直接与线上 Linux
常数比较。

## 已确认的主要调用链

### 普通文件读取

```text
sys_read/readv
  -> acquire_read_lease / PagedPreparedRead::acquire
  -> 分配并清零 staged Vec
  -> GlobalFilePageCache::read_key
  -> install_page（miss 时读取 4 KiB 栈缓冲）
  -> FsPageIo::read_range
  -> StableNodeLease::read_range / FsBridge::read_range
  -> AnotherExt4Fs::read_range_node / Ext4::read
  -> another_ext4 BlockCache
  -> BlockAdapter::read_block（分配 Box<[u8; 4096]>）
  -> SharedBlockDevice spin::Mutex
  -> CachingBlockDevice::read_blocks
  -> VirtIOBlk::read_blocks
  -> VirtQueue::add_notify_wait_pop
  -> while !can_pop() { spin_loop() }
  -> page cache frame -> staged Vec -> copy_to_user -> 用户页
```

关键位置：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs:55-156`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs:638-675,944-985`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:712-831,953-1011`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs:55-91,501-527`
- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs:301-349`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-mmio/src/lib.rs:111-153`
- `os/components/wateros-driver/driver-block/block-impl/impl-virtio-pci/src/lib.rs:324-336`
- `virtio-drivers 0.12.0: src/queue.rs:add_notify_wait_pop`

当前 miss 路径中 VirtIO DMA 直接填充 ext4 `Block` 的缓冲，但随后仍存在
“ext4 Block -> `install_page` 的栈上 `page_buf` -> page cache frame -> staged Vec -> 用户页”
的多级数据搬运，并叠加 page cache、another_ext4 block cache 和 WaterOS LBA block cache
三层缓存。

### 普通文件写入与写回

```text
sys_write/writev
  -> try_kbuf + copy_from_user
  -> detached VFS handle
  -> PagedFileHandle::write
  -> GlobalFilePageCache::write_key
  -> 用户 kbuf -> page cache frame
  -> close/fsync/eviction 时 flush_dirty_run
  -> page cache frame -> batch Vec
  -> FsPageIo::write_range
  -> AnotherExt4Fs::write_range_node
  -> write_with_ordered_size
  -> 扩展写前 flush_all + 写后 flush_all
  -> another_ext4 cache -> BlockAdapter -> WaterOS block cache
  -> VirtIO 同步写 + 忙轮询完成
```

关键位置：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs:331-466`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs:522-557,678-778`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:605-707,1029-1084,1134-1210`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs:137-156,517-533`
- `os/vendor/another_ext4/src/ext4_defs/cache.rs:78-176`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/sync.rs:10-36`

`fsync/fdatasync/sync` 已有显式入口，因此普通 `write()` 不需要为了可见性而每次强制把
整个 another_ext4 cache 刷到块设备。当前策略比 Linux buffered write 的持久化语义强得
多，也是最明确的候选瓶颈之一。

### exec、文件缺页和 COW

```text
execve
  -> load_program_from_path / from_elf_path
  -> 仅预读 ELF header + phdr
  -> 注册 LazyFileVma + ElfPathSegmentLoader
  -> 用户首次访问产生 page fault
  -> handle_lazy_page_fault
  -> 分配并清零用户物理页
  -> ElfPathSegmentLoader::load_page
  -> VFS/page cache/ext4/块设备读取
  -> 拷贝到新用户页
  -> map_page_to_ppn
  -> 单页 flush；外层部分入口还会执行全地址空间 flush/shootdown
```

```text
fork/clone without CLONE_VM
  -> fork_user_aspace
  -> with_user_aspace_mut_and_flush
  -> fork_cow / 递归 fork_table
  -> 为子进程复制页表层级
  -> 对每个私有用户叶增加 frame refcount、修改父 PTE 为 COW
  -> 全本地 TLB flush + 远端 shootdown
```

关键位置：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/execve.rs:28-156`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/clone.rs:220-285`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs:697-760,921-967`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:998-1110,1159-1212,1245-1286`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_aspace.rs:208-228`
- LoongArch 对应文件位于 `impl-loongarch64/src/kernel_elf.rs`、`pagetable.rs`、
  `user_aspace.rs` 的同名函数。

### 异步块 I/O 当前缺失的基础设施

- `SharedBlockDevice = Arc<spin::Mutex<Box<dyn BlockDevice>>>`，同步调用期间设备锁一直被
  持有：`driver-block/block-api/api-v0/src/lib.rs:30-48`。
- `BlockDevice` 只有同步 `read_blocks/write_blocks` trait。
- 上游 virtio-drivers 已有 `read_blocks_nb/write_blocks_nb/peek_used/complete_*`，WaterOS
  尚未使用。
- DTB 已解析 `IrqLine`，但设备注册没有保存并注册 block IRQ：
  `driver-impl/impl-common/src/dtb.rs:40-49`、RV `register.rs:55-109`。
- `trap_handler.rs` 只显式处理 software/timer；`SupervisiorExternel` 会落入 unexpected
  trap：`os/src/trap_handler.rs:325-410`。
- task 层已有 race-safe `WaitQueue::wait_current_while/wake_*`，可以复用：
  `os/components/wateros-task/src/wait_queue.rs:5-71`。

## 已有实验对任务排序的约束

- 保留：稳定 page-cache node key 将 `memcmp` 降低约 76%，完整 RV 从 1076.82s 改善到
  1037.27s。说明稳定 inode identity 和减少字符串键仍有价值。
- 保留：another_ext4 write-back cache、批量 block 释放已显著降低 VirtIO/read_blocks
  和逐块 bitmap/CRC 热点。
- 否决：block cache 8 MiB 扩到 16 MiB 后 VirtIO 约降 5%，但总指令约升 2%。不要继续
  以扩大缓存作为默认方案。
- 否决：TLSF 懒统计实验总指令约升 5.3%。应减少上层分配次数，而非先微调 allocator。
- 否决：两版 mprotect/COW 猜测性优化没有收益，完整轮曾从 1296.63s 退化到
  1410.77s。继续修改前必须先做调用来源、范围和 no-op 比例归因。
- 降级：只有“其他 CPU 空闲”不能证明负载均衡问题。只有忙核 runqueue 中持续存在
  可迁移 runnable task 时，才重新提高负载均衡优先级。

## 任务列表与依赖顺序

### PERF-00：建立可比较的完整基线和低开销分层计数

优先级：P0；所有优化任务的前置条件。

模块/链路：Final runner、`pc-hot`、block/page-cache/ext4/MM 诊断计数。

任务：

1. 用线上相同参数重跑 WaterOS RV 16 GiB/8 vCPU、LA 36 GiB/12 vCPU，均使用
   `-snapshot`。
2. 输出请求数、读写字节、平均请求大小、block/page-cache 命中率、VirtIO notify、
   poll 次数、ext4 `flush_all` 次数、page fault/COW/fork PTE 数。
3. 计数器使用请求内局部变量，结束时一次 Relaxed atomic 汇总；禁止在 poll 内每轮
   atomic add。
4. 诊断由独立 feature 开启，普通 Final feature tree 不包含它。

验收：三次完整轮中位数；计数构建与普通 Final 的 180s 进度差不超过 2%。

### FS-01：去除普通写路径上的全文件系统 `flush_all`

优先级：P0；预计高收益、基础设施要求低，可与 BIO-01 并行。

具体位置：

- `impl-another-ext4/src/lib.rs:137-156` 的 `write_with_ordered_size`。
- 同文件 `close_node/truncate/mkdir/unlink/rename/...` 中的 `flush_all`。
- `paged_handle.rs:522-557,841-843` 的 writeback/fsync 分界。

方案：

1. 推荐：普通 buffered write 只写入 page cache/another_ext4 dirty cache；仅
   `fsync/fdatasync/sync`、dirty eviction、卸载和明确 `O_SYNC` 路径强制写回。
2. 缺少后台 writeback 线程时：先增加 dirty-block 上限，超过上限时由当前写者批量
   writeback，避免无限增长。
3. 若 metadata ordering 尚不完备：先保留创建/rename/unlink 的 metadata flush，单独
   去掉普通数据 write 前后的 `flush_all`，作为低风险过渡。

Linux 参考：buffered write + address_space dirty pages + `writeback/fsync`，但本阶段不复制
jbd2。WaterOS 必须先明确“运行期可见性”和“掉电一致性”是两个契约；没有 journal 时不
宣称等价的 crash consistency。

验收：BuildStorm、iozone/LTP FS、`fsync` 定向测试；快照导出后 `e2fsck -fn`；普通写、
fsync 后读、rename/unlink/open-file 生命周期均正确。若完整轮改善小于 3% 且写放大没有
明显下降，停止扩大改动。

### BIO-01：精确归因 VirtIO 完成忙轮询

优先级：P0；决定是否进入 IRQ 主线。

具体位置：RV/LA VirtIO wrapper 和 virtio-drivers 的 `add_notify_wait_pop`。

方案：

1. 用 pc-hot 原始 PC + 反汇编区分 `can_pop` back-edge、descriptor 分配、notify 和
   `pop_used`。
2. 临时改用 `read_blocks_nb/write_blocks_nb`，仍然轮询，但用局部变量记录每请求
   poll 分布和完成时间。
3. 同时记录请求大小、queue-full、设备锁等待和立即完成比例。

进入 BIO-02 的门槛：poll back-edge 占内核指令至少 5%，或 p95 poll 次数显著大于零，
或估算忙等时间占 BuildStorm 至少 5%。否则优先做请求对象/descriptor 复用和合并。

### BIO-02：建立非阻塞请求生命周期和同步兼容门面

优先级：P0（BIO-01 通过门槛后）；IRQ 和 queue depth 的共同前置。

涉及模块：

- `driver-block/block-api/api-v0`
- `impl-virtio-mmio`、`impl-virtio-pci`
- `impl-block-cache`
- `wateros-task::WaitQueue`

推荐设计：

```text
submit_read/write -> RequestId
RequestState = Submitted | Completed(status) | Reaped
poll_completed -> [RequestId]
wait_complete(RequestId)
同步 read_blocks/write_blocks = submit + wait + reap
```

- request 必须拥有或稳定 pin 住 `BlkReq/BlkResp/DMA buffer/token`。
- 设备锁只保护 queue submit/reap 和 pending table，不得跨任务睡眠。
- 初期仍保留同步 trait 供 ext4 使用，不立即扩大稳定 `api-v0`；异步接口优先留在 block
  聚合/impl 层。
- 使用 `WaitQueue::wait_current_while` 复查完成状态，避免 lost wakeup。

缺少完整 IRQ 基础设施时的过渡方案：非阻塞 submit 后释放设备锁，按有限预算 poll，
预算耗尽后 `yield_now`，其他任务可提交请求。它不是最终方案，但比“持锁 yield”安全，
可以提前验证多个请求在途的收益。

验收：并发读写、queue full、乱序完成、spurious completion、错误完成、任务退出时 pending
request 清理；任何路径都不能持 spin lock 睡眠。

### IRQ-01：RISC-V 外部中断分发和 VirtIO-MMIO completion

优先级：P0/P1；依赖 BIO-02。

涉及位置：

- `driver-impl/impl-common/src/dtb.rs:40-49`
- `driver-impl/impl-qemu-riscv64-virt/src/register.rs:55-109`
- `os/src/trap_handler.rs:325-410`
- RISC-V platform impl 新增 PLIC claim/complete、enable/priority/threshold。

方案：参考 Linux `drivers/block/virtio_blk.c` 的“提交 request、IRQ 回收 used ring、完成
request”结构，但不移植 blk-mq。WaterOS 第一版只需要单个 VirtQueue、pending token 表和
通用 IRQ handler registry。

验收：启动前正确屏蔽，注册后 enable；IRQ 中 ack device、drain 全部 used entry、PLIC
complete；无中断丢失、风暴和重复完成。先以 queue depth=1 证明忙轮询消失和墙钟收益。

### IRQ-02：LoongArch PCI block completion

优先级：P1；依赖 BIO-02 和 IRQ-01 的通用 handler registry，不依赖 RISC-V PLIC 细节。

先确认 QEMU 当前 virtio-pci transport 实际使用 INTx、MSI 还是 MSI-X，再选择最小实现。
若 MSI/MSI-X 基础设施缺失且建设成本过高，阶段一可先使用“有限 poll + 释放锁 + task
wait/yield”的过渡方案；不能为了追求 Linux 完整模型一次性引入通用 PCI IRQ 子系统。

验收：36 GiB/12 vCPU 完整 Final；多 vCPU 并发 I/O；中断 affinity 不应固定造成一个
WaterOS CPU 的 runnable queue 堆积。

### BIO-03：多请求在途、批量 notify 和完成回收

优先级：P1；依赖 BIO-02，IRQ 完成后收益最大。

方案：

1. queue depth 从 1/4/8/16 做 A/B，不直接追求大队列。
2. 连续 block miss 合并已有实现继续保留；进一步允许多个不连续 miss 同时提交。
3. 一批 descriptor 入队后统一 notify；一次 IRQ drain 多个 used entry。
4. page-cache readahead 和 dirty writeback 使用批量 submit，而不是逐请求同步等待。

验收：平均/最大在途深度、notify/request 比值、吞吐和完整轮中位数。若 depth>4 只增加锁
竞争而无墙钟收益，固定较小深度。

### CACHE-01：page-cache miss 单航班和直接填充缓存页

优先级：P0/P1；可独立于 IRQ 推进，之后与 BIO-03 合并。

当前问题：`install_page` 在锁外把磁盘数据读入 `[u8; 4096]`，随后再次锁 cache 并复制到
slot；并发 miss 同一页可能重复读盘。无空闲 slot 时还会 drop lock 后 `spin_loop()`。

方案：

1. 引入 `Free/Loading/Valid/Dirty/Writeback` slot 状态和 generation。
2. miss 时先在锁内预留/钉住 slot，锁外直接把 I/O 读入该 slot，完成后发布 Valid 并唤醒
   同页 waiter。
3. 在 DMA buffer 不能直接使用 page-cache frame 时，先保留一次 bounce copy，但仍通过
   Loading 状态消除重复读和 cache-full 自旋。
4. slot pin/invalidation 语义未建立前，不把裸 slice 跨层长期借出。

Linux 参考：page/folio lock + `filemap_fault`/readahead；WaterOS 只实现单页状态机和 wait
queue，不引入完整 folio/xarray。

验收：同页并发 miss 只产生一次下层 read；eviction/writeback/invalidation 不复用仍在 I/O
中的 slot；取消/错误会唤醒所有 waiter。

### CACHE-02：明确三层缓存职责，减少重复缓存与复制

优先级：P1；依赖 PERF-00 的按来源命中率。

现状：32 MiB file page cache + 约 1 MiB another_ext4 cache + 8 MiB LBA block cache。

候选方案：

1. 保守：file data 由 page cache 管理；another_ext4 cache 重点保留 metadata；LBA cache
   仅缓存 metadata/未被 page cache 覆盖的读取。
2. 中等：给 BlockAdapter 增加调用类别提示（data/metadata），分别统计和选择是否进入 LBA
   cache。
3. 最终：让 file-backed 页直接成为块 I/O 目标，消除 ext4 Block 和 page-cache frame 间的
   一次复制。该方案依赖 CACHE-01 pin 和 BIO-02 buffer lifetime，不能提前做。

不再尝试单纯扩大缓存容量。先用命中率、重复页比例和每层复制字节决定是否关闭某层。

### VFS-01：稳定 inode/dentry/metadata 快速路径

优先级：P1；风险低，适合作为阶段一的持续小步优化。

涉及位置：

- `impl-another-ext4/src/lib.rs:170-260` 的 `lookup_cache`。
- `impl-fs-bridge/src/paged_handle.rs:306-379,560-582`。
- `impl-page-cache/src/lib.rs:50-75,478-527`。

方案：

1. 已打开文件继续全程使用 `(mount_id,inode_id)`，不回退到 path lookup。
2. `openat/stat/access/readlink` 共享一次 pathname walk/metadata 结果。
3. 增加有边界的 negative dentry cache，并在 create/unlink/rename 时精确失效。
4. 将 `lookup_cache` 的“容量满后全 clear”改为有界 LRU/clock，避免 BuildStorm 大目录下周期
   性抖动。

Linux 参考 dcache/namei，但没有 RCU 时优先稳定 node ID + 分片锁/普通有界缓存，不实现
Linux 的 lockless path walk。

### COPY-01：减少 read/write staging 和跨层复制

优先级：P1/P2；阶段一先测，最终目标需要推进。

当前 read 使用 `page cache -> staged Vec -> user page`；write 使用
`user page -> kbuf -> page cache`；writeback 又构造 batch Vec。

方案：

1. read：让 VFS read lease 描述若干 page-cache slice/slot lease，再逐段 copy_to_user，取消
   与请求等长的 staging Vec。
2. write：在保持部分拷贝/EFAULT 语义的前提下，按用户页分段直接写入已 pin 的 page-cache
   slot；基础设施不足时先复用固定大小 per-task/per-CPU staging buffer。
3. writeback：对连续 pinned page 使用 scatter/gather descriptor；不支持 SG 时限制 batch Vec
   并复用容量。

不能用裸用户指针做异步 DMA：用户地址空间可能 fork/exec/unmap，且当前没有 page pin 与
IOMMU 生命周期。第一阶段仍由内核拥有 DMA buffer。

### MM-01：文件缺页复用 page-cache 数据页

优先级：P1/P2；依赖 CACHE-01 的 pin/ref/invalidation 语义。

当前 `handle_lazy_page_fault` 分配并清零新用户页，再通过 ELF loader 从 VFS/page cache 复制
一遍。Linux 的 file-backed fault 通常映射 page cache page。

方案：

1. 最终推荐：只读/可执行私有映射直接引用只读 page-cache frame，并增加 frame refcount；
   首次写入走 COW。
2. 基础设施不足时：loader 一次读取连续多个 fault-adjacent page，批量分配和映射，仍保留
   copy 语义。
3. ELF header/path/segment loader 复用稳定 inode handle，避免 fault 时再次按路径解析。

验收必须覆盖 unlink-open-file、truncate、mmap MAP_PRIVATE/MAP_SHARED、exec、fork COW 和
页缓存 eviction；这些语义未闭环前只能采用方案 2。

### MM-02：按实际修改范围执行 TLB 失效

优先级：P1；中等风险、潜在高收益。

当前多个具体 MM 操作内部已执行 page/all flush，但 `with_user_aspace_mut_and_flush` 无论闭包
是否修改 PTE 都再次执行本地全 flush 和远端 shootdown。fork、COW、mprotect、user copy
可能重复失效。

方案：让闭包返回 `NoChange/Page/Range/All` 修改摘要，由聚合层唯一执行本地和远端 TLB
失效；没有变化时不 flush。先从 COW fault 和 fork 两条可测路径开始，不再次做猜测性的
mprotect 语义改写。

Linux 参考 `mmu_gather`/range flush 的思想，但 WaterOS 使用一个小型 flush descriptor 即可。

### MM-03：fork 页表复制、退出回收与 VMA 元数据批处理

优先级：P2；先计数，避免在进程页集很小时过度设计。

方案：

1. 统计 fork 遍历 PTE、分配页表页、COW 标记和退出释放页数。
2. fork 只复制实际存在的用户页表子树；批量 frame refcount 和一次范围 shootdown。
3. 大地址空间退出的页表/页帧释放可移交 reaper 分批做，但 PID/wait 语义和资源记账必须在
   进程退出点完成。
4. 保持 vfork 快路径；确认 Cargo/rustc 实际 fork/vfork/posix_spawn 比例后再决定投入。

### ALLOC-01：从调用方减少分配，而不是先替换 TLSF

优先级：P2。

候选：复用 `BlkReq/BlkResp`、writeback batch、路径缓冲、iov import buffer；为固定尺寸的
page-cache metadata/request object 建小型池。每项必须先证明 alloc/free 调用数下降。暂不
重新推进已经失败的 TLSF 懒统计或盲目增加 small-object cache。

### SCHED-01：只做单 runnable 快速路径，负载均衡保持门控

优先级：P2。

如果 PERF-00 证明 busy CPU 的 runqueue 长期只有当前任务，则只优化“无需切换时跳过完整
scheduler lock/队列扫描”的快速路径。只有观察到忙核存在多个 runnable task、其他核空闲，
才重新立项 wake placement/负载均衡。

## 推荐执行批次

### 阶段一：目标 `R < 2`

1. PERF-00：取得严格同参数基线和计数。
2. FS-01：去掉普通数据写的每次 `flush_all`，保留显式 durability 边界。
3. BIO-01：确认轮询成本；通过门槛后做 BIO-02 + RV IRQ depth=1。
4. CACHE-01：page miss single-flight，先消除重复读和 cache-full 自旋。
5. VFS-01：稳定 inode/dentry/metadata 小步 A/B。
6. MM-02：消除重复全 TLB flush/shootdown。

每项独立提交、独立完整轮；任何单项完整中位数退化超过 2% 都回退或重新设计。

### 最终阶段：目标 `R <= 1`

1. BIO-03：多请求在途、批量 notify/completion。
2. CACHE-02：按数据/metadata 重新划分三层缓存职责。
3. COPY-01：消除 syscall staging 和 writeback 临时复制。
4. MM-01：只读文件页与 page cache frame 共享、写时 COW。
5. MM-03：依据计数优化 fork/exit 页表生命周期。
6. LoongArch PCI IRQ 和双架构行为/性能对齐。

最终追平 Linux 很可能需要上述跨层组合，单个 block cache 或 allocator 微优化不足以带来
约 60% 的改善。

## 后续 agent 的 CodeGraph 一次性入口

在仓库根目录执行：

```bash
codegraph explore "For the BuildStorm Linux-baseline roadmap, return exact current source and call paths for: sys_read/sys_readv/sys_write/sys_writev; fd SharedIoHandle and prepared read leases; PagedFileHandle read/write/writeback/flush; GlobalFilePageCache install_page/read_key/write_key/flush_dirty_run and FileCacheKey; FsPageIo and StableNodeLease; AnotherExt4Fs lookup/read/write/write_with_ordered_size/flush_all and BlockAdapter; CachingBlockDevice; VirtioBlkDevice MMIO and PCI and virtio-drivers add_notify_wait_pop/read_blocks_nb/write_blocks_nb; DTB IrqLine, external trap dispatch and WaitQueue; execve/from_elf_path/ElfPathSegmentLoader/handle_lazy_page_fault/map_page_to_ppn; fork_user_aspace/fork_table/COW; with_user_aspace_mut_and_flush and TLB shootdown. Include dynamic-dispatch hops, important locks, allocations, copies, sleep/interrupt boundaries, file paths and line ranges."
```

若 CodeGraph 对 cargo registry 依赖不返回源码，再补：

```bash
rg -n "add_notify_wait_pop|read_blocks_nb|write_blocks_nb|complete_read_blocks|complete_write_blocks" \
  ~/.cargo/registry/src -g '*.rs'
```

## 统一验证门禁

- 静态检查：`make check ARCH={rv,la} PROFILE={pre,final}`。
- 构建交付：`make all`，并确认通用 kernel 名称等于 Final 产物。
- 功能：双架构 Final 完整 `BUILDSTORM_COMPILE ok=true`；相关 LTP/BusyBox/IOZone 回归。
- FS：显式 fsync、rename/unlink/open-file、截断、掉电边界说明；镜像副本执行
  `e2fsck -fn`。
- 异步 I/O：queue full、乱序/重复/spurious completion、错误完成、任务退出、设备锁不跨
  wait、lost-wakeup 定向测试。
- MM：fork/exec/COW/mmap/mprotect/munmap、文件 truncate/unlink 后映射、TLB shootdown。
- 性能：同宿主、同 CPU 亲和性、同 QEMU 参数、`-snapshot`、3 次中位数；同时报告 `R`、
  总墙钟、BuildStorm elapsed、指令热点和分层计数。
