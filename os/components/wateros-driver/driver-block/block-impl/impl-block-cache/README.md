# Block Cache 实现手册

本 crate 用写穿缓存装饰任意同步 [`BlockDevice`](../../block-api/api-v0/README.md)，对上仍暴露
相同 trait。块设备分层和平台注册链见 [driver-block](../../README.md)，内核内存预算与缓存层
关系见 [设备、存储、网络与 runtime](../../../../../../docs/offline-development/device-storage-network-runtime.md)。

## 1. 设计目标与非目标

目标：减少根文件系统元数据和重复块读取；连续 cache miss 合并为较少的底层 I/O；顺序扫描
第一次不占 resident cache；写成功后立即更新缓存，使随后读取命中。

当前明确不是：

- write-back cache：没有 dirty bit，flush 不负责遍历缓存行；
- page cache 替代品：这里只认识设备 LBA，不认识 inode/page offset；
- 并发无锁缓存：整个 `CachingBlockDevice` 由外层设备 mutex 串行；
- 可动态缩放缓存：容量和所有辅助表在构造时固定；
- fallible 构造器：大部分预分配走全局 allocator，OOM 可能触发 allocation handler/panic；
- 持久化承诺层：write-through 表示先写后端，但真正稳定介质仍需 `flush()`。

## 2. 源码地图

| 文件 | 职责 |
| --- | --- |
| `src/lib.rs` | `BlockCacheConfig`、feature self-test、host 单元测试和模块导出 |
| `src/device.rs` | resident 数据、LRU、read/write/flush 主流程 |
| `src/index.rs` | 8-way LBA 索引、4-way recent/ghost 索引、诊断计数 |
| `src/manager.rs` | 默认配置、包装成 `SharedBlockDevice`、全设备 flush |

## 3. 核心数据结构

```text
CachingBlockDevice
├─ inner: Box<dyn BlockDevice + Send>       raw backend 所有权
├─ block_size, capacity
├─ data: Vec<u8>                            capacity × block_size
├─ slots: Vec<Slot>                         slot -> LBA + LRU links
├─ free: Vec<usize>                         未占用 slot 栈
├─ map: LbaIndex                            LBA -> slot
├─ lru_head / lru_tail                      已占用 slot 双链
├─ recent: RecentIndex                      只记 LBA，不存数据
└─ diagnostics                              feature 可选计数器

Slot
├─ lba: Option<Lba>
├─ prev: Option<usize>
└─ next: Option<usize>
```

`data` 按 slot 连续分段，slot `i` 的区间是
`i*block_size .. (i+1)*block_size`。`slots` 不持单独 Vec，避免每行一次 allocation。

LRU 头是最久未使用，尾是最近使用。`free` 初始按反序填入，使 pop 从低 index 开始；这只是
实现细节，不能作为 ABI。resident slot 必须同时出现在 map 和 LRU，free slot 必须三项字段
均为 None，且不能在 LRU/map 中。

## 4. 内存预算与 OOM

默认 `BLOCK_CACHE_CAPACITY_BLOCKS=16384`，默认块大小 512，因此仅 `data` 就预分配 8 MiB。
还要加：

```text
capacity * size_of::<Slot>()
capacity * size_of::<usize>()                 free 向量
ceil(capacity/4) * size_of::<[Option<LbaIndexEntry>; 8]>
ceil(capacity/2) * size_of::<[Option<Lba>; 4]>
两个索引的 victim/next 数组与 Vec 元数据
```

精确大小受 Rust 目标 ABI 的 `Option` 布局影响，应在目标构建中用 `size_of` 诊断，不要只按
8 MiB 估算。每注册一块经过包装的物理设备都会付一整份预算；多盘时线性增长。

`new()` 对 `capacity * block_size` 使用 checked_mul，但溢出后把长度设为 `usize::MAX`，随后的
`vec!` 必然尝试巨额分配；其它 slots/index 也预分配。因此配置错误或内核堆不足会表现为全局
heap allocation failure，而不是 `DriverError::NoMemory`。线下遇到 OOM 应先核对：注册设备数、
每设备 capacity、block size、是否重复 probe，以及 page cache/heap 其它大户。

容量 0 会关闭 resident data，但 `LbaIndex::new(0)` 和 `RecentIndex::new(0)` 仍各建立最小一桶；
read/write 主路径透传。底层报告 block_size=0 时也强制 capacity=0，之后请求校验返回 InvalidParam。

## 5. 两个索引

### `LbaIndex`

主索引是 8-way set-associative hash table，bucket 为 `lba % bucket_count`。bucket 数按
`ceil(capacity/4)` 计算：当 resident 满时目标装载率不超过 50%，减少简单模哈希的不均衡冲突。

get 比较完整 LBA，不会因哈希相同返回错误行。insert 先更新同 LBA，再用空 way；bucket 满时
按 `next_victim` round-robin 替换一个索引项并返回 `(old_lba,old_slot)`。调用者必须把被冲突
逐出的旧 resident 从 LRU 摘除、slot 归还 free 并记入 recent，否则会形成不可达 resident 泄漏。

索引冲突淘汰与容量 LRU 淘汰不同：即使还有整体空闲槽，一个极端碰撞 bucket 也可能淘汰旧行。

### `RecentIndex`

recent 是 4-way、只存完整 LBA 的近似 ghost history，bucket 数为 `ceil(capacity/2)`。第一次
read miss 只 insert LBA；第二次 miss 的 `take` 命中才准入 data cache。被 LRU 或主索引逐出的
resident 也 insert recent，因此第一次 refault 可以立刻重新准入。

recent 替换造成 false negative 只会推迟准入；因为比较完整 LBA，不会产生 hash false positive。
它不是访问计数器，也不保证某项保留多久。

## 6. 构造与平台注册链

```text
top-level ARCH feature
  -> driver/impl-block-cache
  -> machine driver 的 block-cache feature

平台 probe virtio-blk
  -> 得到 raw VirtioBlkDevice
  -> BlockCacheManager::default_config
  -> BlockCacheManager::wrap(Box::new(raw), config)
     -> CachingBlockDevice::new：读取 block_size，预分配全部结构
     -> Box<dyn BlockDevice>
     -> Arc<spin::Mutex<...>> = SharedBlockDevice
  -> register_block_device(shared)
  -> devfs alias / rootfs probe
```

必须只包装一次。把已缓存设备再次 wrap 会形成两层互不知情的 block cache，加倍内存并让诊断
失真。当前 manager 无 downcast/去重能力，平台 probe 负责避免重复注册。

## 7. 读取调用链

```text
read_blocks(start, buf)
  -> check_request_range：块大小、对齐、LBA overflow、total_blocks
  -> capacity=0：直接 inner.read_blocks
  -> 从 buf 第 0 块开始扫描
     -> 连续 resident hit：逐块复制 slot_data 到 buf
        -> 只 touch 该 hit run 的最后一个 slot
     -> 连续 miss run：直到下一个 resident hit 或请求末尾
        -> 一次 inner.read_blocks 填充对应 buf 切片
        -> 对 run 中每块 admit_read_miss
           -> recent.take 命中：cache_put_new
           -> 否则：只 recent.insert
  -> diagnostics feature 下按阈值报告
```

底层 read 失败时函数立即返回，失败 run 不进入 cache；buf 之前的 hit/run 可能已经被填充，但
块 trait 返回 Err 后调用者不得把整个输出当成功结果。miss run 合并以遇到 resident 块为边界，
不会为了合并而覆盖已有 cache hit。

第一次顺序扫描只污染 recent 表，不消耗 data slot；完全相同区间第二次读取会从后端再读并
准入；第三次才完全命中。这是有意的 second-hit admission，不是“缓存第一次没工作”。

当前连续 hit run 只把最后一个 hit slot 移到 LRU 尾部，其余命中行不逐个刷新 recency。修改
策略时要先明确这是性能折中还是需要精确 LRU，并更新测试和诊断解释。

## 8. 写入与持久化

```text
write_blocks(start, buf)
  -> check_request_range
  -> inner.write_blocks(start, buf)
     -> 失败：立即返回，resident cache 完全不改
  -> 成功：每块 cache_put（write-allocate）
     -> 已 resident：覆盖 slot，touch LRU
     -> 未 resident：分配/淘汰 slot，复制整块，插主索引与 LRU
  -> 返回 Ok

flush
  -> 直接 inner.flush
```

先 raw write、后 cache update 是最关键的失败原子性：后端拒绝写入时旧 cache 仍表示旧设备内容。
成功后 write-allocate 不需要 second-hit，下一次 read 应命中最新数据。

这仍不是“write 已落盘”：raw 后端可能接受到设备队列/volatile cache，调用持久化边界必须继续
执行 flush。`BlockCacheManager::flush_all()` 遍历全局 block registry，包括未缓存设备；它逐个
持设备 mutex 调用 flush，任一失败立即返回，后续设备不会继续 flush。

## 9. slot 分配、淘汰与自愈

`alloc_slot` 优先 pop free；无 free 时 `evict_lru_slot` 摘除 lru_head、从 slot 取 LBA、删除
map、把 LBA 放 recent，返回可复用 index。

若发现 LRU 为空或 head slot 无 LBA，说明内部不变量已经破坏。当前代码记录 warning，调用
`reset_cache_invariant`：丢弃所有 resident 元数据，重建 map/recent/free，保留 data 字节但使其
不可达，然后重试分配。这选择了“失去命中率但不返回可能错误的数据”。若 reset 后仍没有 slot，
代码会 expect panic；正常 capacity>0 不应发生。

reset 不写后端，因为缓存是 write-through、没有 dirty 数据。若将来实现 write-back，这条自愈
策略会丢脏数据，必须彻底重设计。

## 10. 锁顺序与并发

`SharedBlockDevice = Arc<Mutex<Box<dyn BlockDevice>>>` 的外层 mutex 覆盖 cache 的全部可变状态，
包括等待底层同步 I/O。因此同一设备的 read/write/flush 串行，内部无需额外锁。

推荐调用层锁顺序：

```text
VFS/page cache/inode 先准备请求并尽量释放长生命周期锁
  -> block device mutex
  -> CachingBlockDevice
  -> raw virtio device/queue lock
```

不要持 SHM、task registry、地址空间或可能被 I/O 完成路径反向需要的锁调用块设备。当前实现
没有 task waitqueue；底层 virtio 的轮询/完成语义由具体驱动决定。

`flush_all` 先 clone 单个 registry Arc 再取得设备 mutex，不会持全局 block registry 锁执行 I/O。
新增批量管理函数应保持这个边界。

## 11. diagnostics feature

启用顶层 `cache-layer-diagnostics` 会打开 block-cache diagnostics。计数包括 read hit/miss、
backend read calls/blocks、write/write-allocation、容量淘汰、索引冲突淘汰、ghost hit 和 resident。
每累计约 `1<<20` 个读写块输出一次。

解释时关注比值而非单值：

- miss 高、ghost hit 低：工作集多为一次扫描或 recent 冲突；
- ghost hit 高但 hit 仍低：resident 容量不足/被其它缓存层扰动；
- index eviction 高于 capacity eviction：LBA 模哈希分布冲突，应改 hash/index，而非只加容量；
- backend calls 远小于 miss blocks：连续 miss 合并有效；
- resident 未接近容量但 index eviction 很高：bucket 冲突。

诊断日志位于设备锁内的 read/write 尾部；高频日志会拉长临界区，性能测量时应明确 feature 状态。

## 12. 扩展实例：可失败的动态容量

若要避免启动 OOM并支持运行时调容量，不能只改 `capacity` 字段：

1. 新增 `try_new(...) -> DriverResult<Self>`，所有 Vec 使用 `try_reserve_exact`；
2. checked 计算 payload、slot、bucket 数和总预算，配置上限放入 base-config；
3. 在局部新结构中分配完成，任何失败自动释放，旧设备/缓存不变；
4. 扩容可复制 resident 行并重建 map/LRU/recent；
5. 缩容按 LRU 选保留集，write-through 下可安全丢弃其余行；
6. 只在设备 mutex 内做最终 swap，避免读者看到部分结构；
7. 提供当前/目标内存字节诊断，而非只报告 block 数；
8. 多设备总预算应由 manager 统一限额，避免每设备都拿默认 8 MiB payload；
9. 测试每个 allocation 失败点、容量 0、溢出、缩至工作集以下及并发 I/O。

若进一步做 write-back，需增加 dirty/valid/state、写回错误保留、flush 顺序、barrier/FUA、淘汰
回写和崩溃一致性；它不是在现有 `cache_put` 上加一个 bool 就能安全完成。

## 13. 常见故障定位

| 现象 | 首查 |
| --- | --- |
| 启动/多盘时内核堆 OOM | capacity×block_size、辅助表、设备数、是否重复 wrap |
| 第一次和第二次读都访问设备 | second-hit admission 的预期；第三次才 hit |
| 顺序扫描后热点仍被逐出 | 检查热点是否至少二次访问、recent 冲突和容量 |
| write 成功后 read 旧数据 | cache_put 是否覆盖 resident；是否存在双层/旁路设备句柄 |
| raw write 失败后 cache 却是新数据 | 写入顺序被错误改为 cache-first |
| index eviction 异常高 | LBA modulo 分布、bucket 数/ways，而非 LRU 容量 |
| `alloc_slot` reset warning | map/slot/LRU/free 某条更新路径漏同步 |
| flush 后仍损坏 | 底层 flush/设备错误、FS 写序；本层无 dirty 数据 |
| 大范围 read 后延迟高 | 底层一次大 I/O、设备 mutex 串行、diagnostic 日志 |

## 14. 不变量检查清单

- 每个 map `(lba,idx)` 都满足 `slots[idx].lba==Some(lba)`；
- 每个 occupied slot 恰好在 LRU 一次，head.prev=None、tail.next=None；
- 双链 next/prev 相互指回且无环；
- free 与 occupied 互斥，二者数量之和为 capacity；
- 同一 LBA 最多一个 resident slot；
- slot data 切片始终恰为 block_size 且无乘法越界；
- recent 中的 resident LBA在成功 cache_put 后被 take；
- backend read 成功后才能 admission，backend write 成功后才能更新 cache；
- capacity=0 完全透传且不触碰 slot；
- flush 无论容量是否为 0 都转发 inner。

## 15. 测试与回归

现有 host 单元测试覆盖 second-hit、连续 miss 合并、连续 hit、LRU 刷新、write-through 更新、
write-allocate、容量 0 和索引碰撞。继续修改时至少增加：

- raw read/write/flush 注入失败，确认缓存不被错误提交；
- 请求越界、LBA 加法溢出、零/非标准 block size；
- capacity 1、bucket 极端碰撞、LRU 完整遍历和 invariant reset；
- 多块混合 hit/miss run 的 backend call 边界；
- 多设备内存预算和重复 probe；
- QEMU 根卷 mount、文件读写、fsync、重启后校验；
- BuildStorm/LTP 下 diagnostics 和 heap used/free 趋势；
- RV/LA `make check` 及对应平台实际 I/O。

只通过内存 `CountingMem` 测试不能证明 virtio flush、DMA 可见性或真实文件系统持久化；线下最终
回归必须包含 guest 内端到端写入、flush/卸载/重启和内容校验。
