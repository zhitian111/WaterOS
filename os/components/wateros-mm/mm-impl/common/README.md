# MM 架构公共实现离线开发手册

[MM 总览](../../README.md) · [MM API](../../mm-api/api-v0/README.md) · [Sv39 实现](../impl-sv39/README.md) · [LoongArch64 实现](../impl-loongarch64/README.md)

`common` 保存 Sv39 与 LoongArch64 共用、且不应知道 PTE 位编码、页表层数、ASID
格式或 TLB 指令的算法。它不是稳定公共 API：对外语义仍以 `mm-api/api-v0` 为准，
修改这里必须同时回归两套架构实现。

本文按当前源码描述真实行为。标为“当前限制”的内容不是 Linux 应有语义，而是线下
修复时需要优先检查的边界。

## 1. 模块边界与代码地图

| 文件 | 主要对象 | 负责 | 不负责 |
| --- | --- | --- | --- |
| `src/lib.rs` | `ZeroAnonLoader`、`ElfSegmentLoadParams` | 公共导出、匿名零页 loader、ELF 单页填充 | 页表安装和 TLB |
| `src/vma.rs` | `VmaBacking`、四种 VMA、`LazyVmaSet` | lazy VMA 查找、切分、权限和 backing 复制 | 完整地址空间 VMA 注册表 |
| `src/fault.rs` | `LazyVmaAccess`、`handle_lazy_file_fault` | lazy 文件/匿名缺页的内容和装页策略 | COW、共享匿名页、架构 flush |
| `src/mapping.rs` | 映射辅助函数、`mremap_range` | 清零、eager 映射、空洞扫描、mremap 数据复制 | lazy VMA 元数据迁移 |
| `src/cache.rs` | ELF/mmap 只读页缓存 | 跨地址空间共享不可写文件页 | 可写页缓存、主动失效/回收 |
| `src/elf.rs` | ELF 小端读取和一致性检查 | ELF 头快速校验、重复读择优 | 完整 ELF 合法性和重定位 |

架构实现仍拥有根页表、页表中间页、ASID、mmap 游标、共享匿名/文件 VMA、设备
VMA、lazy VMA、COW、销毁写回和 TLB shootdown。不要把 `LazyVmaSet` 误当作进程
的全部虚拟内存描述。

## 2. 核心数据结构

### 2.1 `VmaBacking`

```rust
pub enum VmaBacking {
    Anonymous,
    File { loader: Box<dyn DemandPageLoader> },
}
```

- `Anonymous`：fault 前目标页已清零，`load_page` 什么也不做；`write_page` 返回
  `Unsupported`；`flush` 是成功空操作。
- `File`：把 `duplicate_box/load_page/load_shared_page/write_page/flush` 委托给 loader。
- `duplicate()` 只复制 loader 外壳。底层打开文件、内容身份和锁是否共享，由 loader
  自己决定，通常用 `Arc`。
- loader 是可变对象，因此 fault/writeback 要取得 VMA 的可变借用。loader 回调不可
  重入同一地址空间锁。

`DemandPageLoader` 的关键契约：

1. `load_page(offset, dst)` 接收一个已经清零的页，只需填文件有效区。
2. `load_shared_page` 若返回 PPN，已替调用者持有一个帧引用；PTE 安装失败必须释放。
3. `write_page/flush` 用于共享文件映射；只读/私有映射可以保持默认实现。
4. `duplicate_box` 失败会使 fork 或 VMA 切分失败，调用者必须保证元数据不丢失。

### 2.2 `LazyFileVma`

```text
[start, end)          页对齐虚拟范围
perm                  fault 时同时要求 U 和对应 R/W/X
file_offset           start 对应的 backing 字节偏移
file_size             映射创建时记录的文件大小快照
backing                Anonymous 或 File loader
```

缺页地址 `page` 对应：

```text
offset = vma.file_offset + (page - vma.start)
```

`file_size` 不随左右切分缩小；loader 结合 `file_offset` 和该快照决定文件尾之外保持零。
`contains_page` 使用半开区间，`overlaps` 使用标准条件
`a.start < b.end && a.end > b.start`。

### 2.3 其他 VMA

- `SharedAnonVma { start, end }`：标记叶 PPN 由外部共享对象持有，普通 unmap 不能把它
  当独占帧释放。
- `SharedFileVma { start, end, file_offset, backing }`：保留共享映射写回 loader；权限
  存在驻留 PTE 中，不在此结构里。
- `DeviceVma { start, end, phys_start, perm, lease }`：物理页不归通用帧分配器；
  `Arc<dyn DeviceMappingLease>` 保证 DMA/framebuffer 等底层对象在映射期间存活。
- 这三类当前由架构实现中的 `Vec` 管理，不属于 `LazyVmaSet`。

### 2.4 `LazyVmaSet`

内部是 `Vec<LazyFileVma>`，设计不变量是按 `start` 升序且互不重叠。

| 操作 | 行为 | 复杂度 |
| --- | --- | --- |
| `lookup(page)` | 按 `end` 二分，失败再线性兜底 | 通常 O(log n)，失序时 O(n) |
| `overlaps/overlap_end` | 找第一个可能相交项 | O(log n)，后者有线性兜底 |
| `protect_range` | 最多产生左右两个切片，中段替换权限 | O(n) |
| `merge_perm` | 对所有重叠项把权限按位 OR | O(n) 加 loader 复制 |
| `remove_range` | 删除中段，保留左右切片 | O(n) 加 loader 复制 |
| `replace` | 排序后整体替换 | O(n log n) |

`merge_perm(start,end,perm)` 是 `old | perm`，用于 ELF 多个 `PT_LOAD` 落在同一页；
`protect_range(start,end,perm)` 才是直接替换，供 `mprotect` 使用。

当前限制和修复注意点：

- `insert(index,vma)` 不会自动排序或检查重叠，调用者必须先算正确下标；只有 `sort()`
  会执行 debug 下的无重叠断言。
- `from_vec` 会排序并在 debug 构建检查重叠；`replace` 只排序，不检查重叠。
- release 构建没有运行时不变量校验；重叠 VMA 的命中结果取决于顺序。
- `merge_perm/remove_range` 在 `drain(..)` 中调用 `duplicate()`。若 loader 复制中途失败，
  原集合已被破坏，尚未处理的 drain 项也会被移除；这两个操作当前不是失败原子事务。
  修复时应先完成全部 fallible 复制，再一次提交新 `Vec`。
- 切分后不会自动合并相邻且 backing 等价的 VMA，反复 `mprotect/munmap` 会增加条目数。

## 3. Lazy 缺页完整调用链

```text
用户 load/store/fetch 触发页故障
  -> arch trap 解码为 PageFaultAccess::{Read,Write,Execute}
  -> AddressSpace::handle_page_fault
       -> handle_lazy_page_fault
            -> common::handle_lazy_file_fault
                 -> LazyVmaSet::lookup(floor_page(fault_addr))
                 -> 检查 PagePerm 对应 R/W/X 且 U=1
                 -> translate_addr(page)
                    已有叶 PTE：Ok(true)，可能是并发 CPU 已安装
                 -> offset = vma.file_offset + page - vma.start
                 -> 不可写 VMA：backing.load_shared_page(offset)
                    Some(ppn) -> map_page_to_ppn
                 -> 否则 alloc_zeroed_frame_with_alloc
                 -> backing.load_page(offset, dst)
                 -> map_page_to_ppn
            -> arch local page TLB flush
       -> 若仍未处理，再尝试 COW/其他 fault 分类
       -> 最终转用户 SIGSEGV/SIGBUS 或 syscall 错误
```

返回语义：

- `Ok(true)`：lazy fault 已处理，调用者必须失效本 CPU 的旧 TLB 项后重试指令。
- `Ok(false)`：没有 VMA，或访问权限/用户位不允许；这不是内部错误。
- `Err(e)`：查页表、分配、加载或安装失败；上层决定信号/errno。

引用和失败路径：

- 缓存共享页：loader 给出“映射引用”；安装失败调用 `frame_dealloc_result`。
- 新分配页：load 或安装失败调用传入 allocator 的 `dealloc_frame`。
- 已存在 PTE 的竞态路径不增减引用，只要求调用者 flush stale TLB。

common 没有页级 busy 状态。跨地址空间对同一只读文件页的并发 miss 允许重复 I/O，
由缓存发布阶段消重。同一地址空间的并发由上层 task/mm 锁序约束。

## 4. 清零与 eager 映射辅助函数

### 4.1 物理页访问假设

`zero_phys_page`、`fill_phys_page`、loader 和 mremap 都把 `ppn * PAGE_SIZE` 直接转成
内核指针。这依赖当前恒等直接访问模型；以后引入高端内存或非恒等 direct map 时，必须
统一改成 `phys_to_virt` 窗口，不能只改某一个 helper。

`alloc_zeroed_frame_with_alloc` 先尝试 allocator 的预清零快速路径，返回 `None` 才普通
分配并手工清零。

### 4.2 映射函数的事务性

| 函数 | 内容来源 | 失败后的当前状态 |
| --- | --- | --- |
| `map_zeroed_page_with_alloc` | 零页 | PTE 安装失败时没有主动释放新帧 |
| `map_zeroed_range_with_alloc` | 多个零页 | 中途失败时已装页保留，也没有整段回滚 |
| `map_range_from_backing` | 内存 slice | 中途失败不回滚；安装失败的新帧也未显式释放 |
| `map_range_from_loader` | 回调逐页加载 | 释放当前帧，并尝试 unmap 已完成前缀 |

只有 `map_range_from_loader` 明确尝试前缀回滚，而且回滚错误被忽略。新增 syscall 不要
假设 helper 自动保证“失败不留副作用”。可靠方案应引入映射事务，记录已取得的 PPN/PTE，
在 Drop 或显式 rollback 中逆序撤销。

### 4.3 mmap 空洞搜索

`find_free_mmap_base(aspace,cursor,len)` 拒绝零长度，将 cursor 向上页对齐，逐候选页用
`translate_addr` 检查，冲突后前进一页，最多跳过 `1 << 20` 页（4 GiB 窗口）。

它只看驻留叶 PTE。尚未 fault 的 lazy VMA 没有 PTE，会被误判为空洞；两架构必须调用
自己的 `find_free_mmap_base_considering_vmas`，同时避开 lazy/shared/device VMA。新增
VMA 类后也必须加入包装层，否则下一次 mmap 会覆盖它。

## 5. `mremap` 当前语义与危险边界

```text
MREMAP_MAYMOVE   = 1
MREMAP_FIXED     = 2，必须同时有 MAYMOVE
MREMAP_DONTUNMAP = 4，当前实现要求同时有 FIXED
```

当前只面向匿名私有映射和“恰好由一个完整 lazy VMA 覆盖”的有限场景。共享匿名映射
拒绝搬迁；跨多个 lazy VMA、混合权限或部分 VMA 会返回 `Unsupported`。

```text
sys_mremap
  -> with_user_aspace_mut_and_flush
  -> arch AddressSpace::mremap
       -> 对齐范围，验证 VMA 类型和统一权限
       -> 判断原位增长或选择 relocation_base
       -> common::mremap_range
            shrink: unmap 尾部
            grow: 为尾部新分配零页
            move: 新区分配 -> 逐页复制 -> 可选 unmap 旧区
            FIXED: 先 unmap 目标 -> 分配 -> 复制 -> 可选 unmap 旧区
       -> arch 迁移/切分 lazy VMA 元数据
  -> 统一 TLB flush
```

与 Linux 完整语义的差异及故障点：

- 未拒绝非页对齐 `old_addr`；内部向下取整，缩小时可能仍返回原始非对齐地址。
- 移动不是转移 PTE，而是新分配并复制，峰值物理内存约为旧区加新区；大映射易 OOM。
- `MREMAP_FIXED` 在确认新映射成功前先 unmap 目标，失败不会恢复原目标内容。
- 新区分配、复制、旧区 unmap 任一步失败都可能留下半新区或半旧区；没有完整 rollback。
- `copy_mapped_bytes` 用 `copy_nonoverlapping`；固定目标显式拒绝与旧区重叠。
- common 只移动驻留内容，lazy VMA 元数据由架构函数事后调整；该步失败会使两者不一致。
- `DONTUNMAP` 的组合约束是 WaterOS 子集，不代表完整 Linux 行为。

修复顺序应是：纯参数/覆盖类型校验，预留目标及全部帧，原子提交 PTE/VMA，最后释放
旧区；提交前任何失败均释放预留资源，提交阶段不可再做 fallible loader 复制。

## 6. 只读页缓存

### 6.1 key 与所有权

- ELF cache key：文件身份、内容版本、`vbase/p_offset/filesz/vma_start/file_offset`。
  同一文件字节在非页对齐段中的落位可能不同。
- private readonly mmap key：文件身份、内容版本、页偏移和 `mapping_file_size`；文件尾
  零填充取决于创建映射时的大小快照。

稳定身份是 `mount_generation + mount_id + node_id + Arc<AtomicU64> version`。VFS 内容
变化必须调用 `mark_changed()`。mount generation 变化产生新 key，但不会删除旧 key。

### 6.2 hit/miss/并发发布

```text
读取 version -> 组 key -> cache spin lock 查 BTreeMap
  hit: frame_inc_ref，更新 last_used，解锁
  miss: 解锁，分配/清零页，执行文件 I/O
        version 变化 -> 释放并重试
        再加锁发布
          同 key 已发布 -> 增缓存页映射引用，释放重复加载页
          否则插入 loaded_ppn，再增加缓存持有的引用
        解锁并再次检查 version
```

文件 I/O 不在 cache spin lock 内执行。`_identity` 保留 identity/version token 生命周期；
缓存持有一个 PPN 引用，每个 PTE 再持一个。

### 6.3 容量和回收现实

| 缓存 | 容量 | 满时策略 | 仅页帧上限 |
| --- | ---: | --- | ---: |
| ELF readonly | 16,384 页 | O(n) 扫描 `last_used`，淘汰一个 | 64 MiB |
| mmap readonly | 32,768 页 | 不淘汰；新 miss 绕过缓存 | 128 MiB |

还需加 BTreeMap 节点和 identity `Arc` 的堆开销。当前无 shrinker、清空接口或内存压力
回调：

- ELF 满后每个新 key miss 都在 spin lock 内线性扫描 16K 项。
- mmap 满后不再接纳新热点；旧 version/mount generation 项一直占槽并持帧至重启。
- 理论合计约 192 MiB 页帧，是排查 512 MiB 内核堆/物理内存压力时必须计入的常驻量；
  它们不是进程退出时必然回收的 RSS。
- `cache-layer-diagnostics` 每 `1<<14` 次 lookup 打印统计，当前使用 error 级日志。

理想改造是统一可回收 page-cache/shrinker，按压力回收仅有 cache 引用的帧，并使用哈希
或分段锁降低全局 BTreeMap spin lock 争用。

## 7. ELF 页装载和读取稳定化

### 7.1 单页填充

`fill_elf_load_page` 计算当前页与 `[vbase,vbase+filesz)` 的交集，只读取交集对应文件
字节。调用者必须先清零 `dst`，段前空隙和 BSS 才自然为零。

`ElfSegmentLoadParams` 用
`page_va = vma_start + file_offset.saturating_sub(vma_file_origin)` 还原虚拟页地址。异常的
小 offset 会被静默夹到 VMA 起点；新增 loader 最好在构造时证明 offset 不变量。

### 7.2 快速校验与稳定读

- `rd_u16/u32/u64`：小端读取，普通越界返回 `None`。
- `elf_entry_plausible`：要求非零 entry 落在某个非空 `PT_LOAD` 内。
- `entry_file_offset`：将 entry PC 转成所属段的文件偏移。
- `finalize_elf_read`：文本/非 ELF 不重读；ELF 读第二次，不一致再读第三次，以相同且
  entry 可信者优先；三份都不可信时返回第二份交给后续 parser 报错。

它不是完整 ELF verifier，也不是文件快照事务。部分 `e_phoff + i * e_phentsize` 运算
尚未全部 checked，恶意畸形 ELF 在 debug 构建可能整数溢出 panic，release 下可能绕回；
安全加固时应逐项改为 `checked_mul/checked_add`。

## 8. 新增 mmap 类 syscall 实例

以新增 `mmap_populate` 风格功能为例：

1. ABI 层检查地址、长度、flag 组合并转换为 `VirtAddr/PagePerm`。
2. 通过 task/mm 注册表取得当前 `UserAddressSpaceHandle`。
3. 用 `with_user_aspace_mut_and_flush[_if_changed]` 进入地址空间锁域。
4. 在架构 `MmapOps` 实现中检查所有 VMA 类型，选址并预构造元数据。
5. 用可回滚事务逐页 populate；失败时恢复 PTE、帧引用和 VMA。
6. 提交后返回 `PteChange`/布尔变更，让包装层执行正确 TLB flush。
7. syscall 层用统一 `mm_err_to_errno` 转换错误。

```rust
match mm::user_aspace::with_user_aspace_mut_and_flush_if_changed(handle, |aspace| {
    let mut alloc = GlobalPhysFrameAllocator;
    let changed = aspace.populate_range(&mut alloc, VirtAddr(addr), len)?;
    Ok(((), changed))
}) {
    Ok(()) => UserRet::from_success(0),
    Err(error) => UserRet::from_error(mm_err_to_errno(error)),
}
```

若只改 lazy VMA、不改变驻留 PTE，可返回 `changed=false`；撤销、替换或改变叶权限必须
flush。跨 CPU shootdown 由上层活动 CPU 协议处理，不能在 common 里只做 local flush。

## 9. 锁、生命周期与帧所有权

推荐锁序：

```text
task/user-aspace registry
  -> 单个 AddressSpace 可变访问
     -> VMA/页表
        -> 短暂 readonly-cache spin lock（绝不做 VFS I/O）
```

loader 的 `load_page/write_page/flush` 会进入 VFS、文件系统和块设备，不要持有会被 VFS
反向获取的 inode/page-cache spin lock。地址空间销毁时共享文件写回可能失败；显式
`msync/munmap/close` 应传播错误，Drop 只能记录时须打印 identity、offset 和首个错误。

| 页类型 | PTE 删除时 | VMA 删除时 | 额外所有者 |
| --- | --- | --- | --- |
| 普通匿名/private | `dealloc_frame` | 仅删元数据 | fork COW 引用 |
| readonly cache | 释放映射引用 | 仅删元数据 | 全局 cache 引用 |
| shared anon/file | 只删 PTE，走外部协议 | 写回/删元数据 | SHM/backing |
| device | 只删 PTE | drop lease | 驱动/设备对象 |

## 10. 故障定位

### fault 后反复进入同一地址

检查 VMA 命中、`U/R/W/X`、PTE valid、安装后 local flush、ASID、远端 shootdown。
`Ok(true)` 不代表 common 已经 flush。

### mmap 覆盖尚未访问的区域

检查是否误用 `find_free_mmap_base`，以及架构包装层是否漏查新增 VMA。lazy VMA 没有
PTE，单看 `translate_addr` 必然看不见。

### fork/munmap/mprotect 后 VMA 消失

注入 `duplicate_box` 失败。`merge_perm/remove_range` 当前在 drain 中失败会破坏集合；也
检查是否用错误 index 调用了不会排序的 `insert`。

### 压力测试 OOM，但进程 RSS 已退出

分别记录物理帧引用、内核 heap、ELF/mmap cache resident、旧 version key、页表中间页、
task/fd/VMA 对象。只读缓存最高约 192 MiB 且无压力回收，不能只看 `/proc/meminfo`。

### 地址空间销毁写回失败

沿 `SharedFileVma -> VmaBacking::write_page/flush -> DemandPageLoader -> VFS handle` 定位。
记录 mount generation、mount/node id、content version、file offset、失败页和 errno。检查
mount generation/cache flush 是否在 dirty 页写回成功前推进，以及是否发生锁反转。

### mremap 后旧区/新区半残

逐阶段确认目标预先 unmap、帧分配数、复制失败点、旧区 unmap 进度、VMA 元数据和最终
TLB flush。common 当前不是事务，修复必须加入 fault injection。

## 11. 自回归测试矩阵

- `LazyVmaSet`：空集、边界、相邻、重叠、中部切分、跨 VMA protect、duplicate 第
  1/N 次失败后的集合完整性；
- fault：无 VMA、权限拒绝、匿名零页、文件尾零填充、缓存共享、load/map 失败、peer
  已安装；
- refcount：cache+两个映射为 3，安装失败回到 cache-only，fork/unmap 逐次下降，设备
  页永不进入通用 allocator；
- mmap：零长度、溢出、不对齐游标、只存在 lazy VMA 的洞、4 GiB 搜索上限；
- mremap：shrink、原位 grow、冲突无 MAYMOVE、move、FIXED、DONTUNMAP、重叠、混合
  权限/共享 VMA 拒绝，以及每个分配/复制/unmap 失败点；
- cache：同 key、版本变化、并发 duplicate publish、容量满、ELF 淘汰、mmap bypass、
  mount generation 变化；
- ELF：非页对齐 PT_LOAD、BSS、入口边界、畸形大 phoff/phnum、三次读组合；
- 生命周期：`exec/fork/exit/munmap/msync` 和 Drop 写回成功/失败。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
git diff --check
```

`self_test` 当前只覆盖 `ZeroAnonLoader` 和两个只读缓存的基本 key/refcount，不覆盖 VMA
失败原子性、mremap rollback、并发 TLB 或销毁写回；它通过不能替代上述矩阵。

## 12. 修改检查清单

- [ ] 新 VMA 类型已加入选址、fault、fork、mprotect、munmap、mremap、exec/exit/Drop。
- [ ] 明确 PPN 所有者；每条成功/失败路径的增减引用成对。
- [ ] 所有长度、偏移和页数运算使用 checked arithmetic。
- [ ] fallible loader 复制发生在提交前，失败不破坏旧 VMA 集合。
- [ ] 文件 I/O 不在 cache spin lock 或不可重入页表锁内。
- [ ] VFS 写入/截断推进 content version，mount 变化不遗留无限旧缓存。
- [ ] 区分只改元数据与改驻留 PTE，TLB flush 范围正确。
- [ ] 两架构与 common 契约同步，并完成 RV/LA 回归。
