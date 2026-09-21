# MM 架构公共实现离线开发手册

[MM 总览](../../README.md) · [MM API](../../mm-api/api-v0/README.md) · [Sv39 实现](../impl-sv39/README.md) · [LoongArch64 实现](../impl-loongarch64/README.md)

`common` 保存 Sv39 与 LoongArch64 共用、且不应知道 PTE 位编码、页表层数、ASID
格式或 TLB 指令的算法。它不是稳定公共 API；对外契约仍以 `mm-api/api-v0` 为准。

## 1. 模块边界

| 文件 | 主要对象 | 职责 |
| --- | --- | --- |
| `src/vma.rs` | `VmArea`、`VmaSet`、`VmaBacking` | 地址空间唯一 VMA 模型、查找、切分、权限更新、合并 |
| `src/fault.rs` | `VmaAccess`、`handle_vma_fault` | 按统一 VMA 完成权限检查和按需装页 |
| `src/mapping.rs` | 映射辅助函数、`mremap_range` | 清零、eager 映射、mremap 驻留页迁移 |
| `src/cache.rs` | ELF/mmap 只读页缓存 | 跨地址空间共享不可写文件页 |
| `src/elf.rs` | ELF 读取辅助 | ELF 头快速校验与稳定读取 |

架构实现持有根页表、ASID、地址游标和一个 `VmaSet`。页表只记录已经驻留的硬件
映射、A/D 位与 COW 状态；区间是否存在、权限、private/shared、用途、按需策略和
backing 均以 VMA 为准。

## 2. Linux 风格的统一 VMA

`VmArea` 描述页对齐半开区间 `[start, end)`：

```text
range          start / end
access         PagePerm
sharing        Private / Shared
kind           Anonymous / File / Heap / Stack / SharedMemory / Device
fault policy   demand_paged
file mapping   file_offset / file_size
backing        Anonymous / File(loader) / External / Device(phys_start, lease)
```

同一虚拟页最多属于一个 VMA。过去的 lazy file、shared anon、shared file 和 device
并行表已删除；共享文件映射也不再同时登记成 shared-anon。`Heap`、`Stack`、eager/lazy
ELF `PT_LOAD`、匿名/文件 mmap、SysV SHM 与设备映射都进入相同集合。

`VmaBacking` 的语义：

- `Anonymous`：fault 得到预清零页；
- `File { loader: Some(_) }`：可按需读取、取得只读共享页或执行共享写回；
- `File { loader: None }`：已经 eager 驻留的文件段，仅保存区间来源，不允许重新装页；
- `External`：SysV SHM 等离散外部页，registry 持有物理页，VMA 只记录 non-owned 语义；
- `Device`：保存连续设备 PPN 起点和生命周期 lease，不由通用帧分配器回收。

`VmaSet` 内部使用 `BTreeMap<start, VmArea>` 建立范围索引，并在运行时拒绝零长、非页对齐和
重叠插入。由于 VMA 全局无重叠，包含某地址的条目就是 `..=addr` 的最后一个节点；这使点查询
与首个冲突定位为 `O(log n)`，相交区间遍历为 `O(log n + k)`。它借鉴 Linux Maple Tree 的
“单一有序范围索引”模型，但不是 Linux 支持 RCU 的完整 Maple Tree，也不是经典线段树。
主要操作为：

| 操作 | 语义 |
| --- | --- |
| `lookup` / `overlap_end` | 在 B-tree 中查找唯一 VMA 或第一个冲突区间 |
| `covers` | 验证范围是否由无洞的相邻 VMA 完整覆盖 |
| `remove_range` | 删除中段并保留左右片段，同时修正文件偏移/设备 PPN |
| `protect_range` / `merge_perm` | 切分区间后替换权限或按位合并权限 |
| `duplicate_subrange` | 为 fork/mremap 复制一个 VMA 子区间及 backing |

切分所需的 fallible loader 复制和偏移检查在替换旧集合前完成。相邻且语义相同的匿名
VMA 会自动合并。文件 backing 尚无可比较的稳定对象 identity，因此 eager 装载时直接按
连续段建立 VMA，而不会仅凭“偏移连续”跨文件合并；带 loader 或设备 lease 的 VMA 也不
自动合并。

## 3. 缺页、COW 与 PTE

```text
用户 page fault
  -> arch trap 解码 PageFaultAccess
  -> AddressSpace::handle_page_fault
       -> handle_vma_page_fault
            -> common::handle_vma_fault
                 -> VmaSet::lookup(page)
                 -> demand_paged + U/R/W/X 校验
                 -> File/Anonymous backing 装页
            -> 架构 local TLB flush
       -> COW 写故障路径（若适用）
       -> 未处理则交给信号/错误路径
```

`Ok(true)` 表示 VMA fault 已处理或该页已经由并发 CPU 安装；调用方仍须处理本地旧 TLB
项。`Ok(false)` 表示没有对应的按需 VMA或权限不允许。设备与 eager VMA 不会在 fault
路径凭区间重新生成页面，因此 `munmap` 堆/栈后不会被旧的边界字段“复活”。

fork 时先复制 `VmaSet`，再根据每页所属 VMA 决定：shared 映射保持共享，private 可写页
进入 COW，External/Device 页只复制 PTE，不增普通 frame 引用。内核写用户页前也查询同一 VMA，避免把
shared 页误做私有复制。

## 4. 映射生命周期

- ELF：lazy `PT_LOAD` 直接登记带 loader 的 `File` VMA；eager `PT_LOAD` 在装载时登记
  `File { loader: None }`，其零填充尾部仍属于该装载段，重叠段页合并权限。
- `brk`：字节级当前端点仍保存在 layout 字段中，实际已覆盖页由 `Heap` VMA 管理。
- 用户栈：固定布局字段用于边界/选址，实际页与按需缺页由 `Stack` VMA 管理。
- mmap：匿名、文件、共享、SysV SHM 和设备映射都先确定统一属性，再插入同一集合。
- `mprotect`：要求范围由 VMA 完整覆盖；未驻留页只更新 VMA，驻留页同时改 PTE。
- `munmap`：先按 VMA 所有权解除 PTE，再对集合做一次统一区间删除。
- `mremap`：复制单一 VMA 内的源子区间；shared/device/heap/stack 仍返回 `Unsupported`。
- destroy：共享文件先按 VMA 写回，然后页表销毁按 VMA 判定普通 frame 或 external/device 所有权。

`MREMAP_FIXED` 和底层多页映射目前仍没有完整 PTE 事务：目标预先 unmap 后若分配/复制
失败，不能恢复旧目标。这是驻留页提交层的现有限制，不再是多张 VMA 表不同步问题。

## 5. 帧与 backing 所有权

| 页类型 | PTE 删除 | VMA 删除 |
| --- | --- | --- |
| private anonymous/file | 释放映射帧引用 | drop 元数据/loader |
| readonly cache | 释放映射引用，缓存保留自身引用 | drop loader |
| shared anonymous/file | 释放当前地址空间的帧引用；fork 维持引用计数 | shared file 先写回，再 drop loader |
| SysV SHM external | 只断开 PTE；普通 `munmap`/`MAP_FIXED` 不得绕过 registry | `shmdt` 后 registry 更新 attachment/nattch |
| device | 只断开 PTE，不进入通用 allocator | 最后一个片段释放 lease |

loader 的 `load_page/write_page/flush` 可能进入 VFS 与块设备。调用方持有地址空间可变访问，
但不得同时持有会被 VFS 反向获取的 inode/page-cache spin lock。

## 6. 只读页缓存

ELF cache key 包含文件身份、内容版本和段内落位；private readonly mmap key 包含文件
身份、版本、页偏移与创建映射时的文件长度。缓存 I/O 不在 cache spin lock 内执行。

当前 ELF readonly 缓存最多 16,384 页并用线性 LRU 扫描淘汰；mmap readonly 缓存最多
32,768 页，满后新 miss 绕过缓存。两者合计页帧上限约 192 MiB，尚无统一 shrinker。

## 7. 调试重点

### mmap 覆盖未 fault 区域

确认选址调用 `find_free_mmap_base_considering_vmas`，并检查 `VmaSet::overlap_end`。单看
PTE 无法发现尚未驻留的 VMA。

### fault 后反复进入同一地址

检查 VMA 的 `demand_paged`、U/R/W/X、backing loader、PTE 安装、ASID 和 TLB 失效。

### fork/munmap/mprotect 后语义错误

打印命中的唯一 `VmArea`，重点核对 `sharing/kind/demand_paged`、切分后的
`file_offset` 或 `phys_start`。不再需要交叉比对多张区间表。

### 地址空间销毁写回失败

沿 `VmArea(File+Shared) -> VmaBacking::write_page/flush -> DemandPageLoader -> VFS`
定位，记录 VMA offset、失败页、文件身份和 errno。

## 8. 回归矩阵

- `VmaSet`：乱序插入、相邻合并、重叠拒绝、未对齐拒绝、切分、权限切分、文件偏移、
  external non-owned、设备 PPN、loader 复制失败原子性；
- fault：无 VMA、非 demand VMA、权限拒绝、匿名零页、文件尾零填充、缓存共享；
- 生命周期：`exec/fork/exit/brk/mmap/munmap/mprotect/mremap/msync`；
- 所有权：private COW、shared fork、SysV SHM external、设备 lease、只读 cache refcount；
- 架构：RISC-V Sv39 与 LoongArch64 默认配置、`elf-lazy-map` 开/关组合。

```bash
cd os
make rv_check
make la_check
git diff --check
```

## 9. 修改检查清单

- [ ] 新映射类型进入唯一 `VmaSet`，没有新增平行区间表。
- [ ] 区间始终页对齐、有序、无重叠，切分同步修正 backing 起点。
- [ ] fault、fork、mprotect、munmap、mremap、destroy 查询同一 VMA 语义。
- [ ] PPN 所有者明确，成功/失败路径引用成对。
- [ ] fallible loader 复制发生在元数据提交前。
- [ ] 两架构实现和 eager/lazy ELF 路径同步验证。
