# 性能优化：内存管理（帧分配 / 页表 / 内核堆 / mmap / COW / TLB）

## 用途

汇总 `wateros-mm` 与 `runtime-heap-allocator` 中的性能瓶颈与资源回收/flush 隐患，重点是：TLB 刷新策略、fork COW、地址空间销毁与中间页表帧回收、brk/mmap 懒加载、帧分配器与内核堆算法。这些是内核内存子系统的热路径与「资源只减不增」泄漏源。

## 事实来源

- 代码静态链路分析（RV=Sv39，LA=LoongArch64 三级页表）。
- 关联子链路分析见 [memory-subagent](09ce5359-c553-46ad-8db4-30888ce225e1)。
- 交叉参考：`docs/audits/resources/physical-frames.md`、`docs/audits/resources/kernel-heap.md`、`docs/audits/locks/mm-allocators.md`、`docs/audits/resource-inventory.md`（已点名 fork COW、mmap 懒加载、MAP_SHARED 不回收等）。

## 覆盖范围

`os/components/wateros-mm/mm-frame-alloctor`、`mm-impl/impl-sv39`、`mm-impl/impl-loongarch64`、`mm-impl/common`、`mm-api/api-v0`、`os/components/wateros-runtime/runtime-heap-allocator`。

> 注：与 `perf-hotpath.md` 的 H-1/H-4（trap 往返与 TLB 全局 flush）部分重叠，本文档侧重 MM 层 PTE 修改后的 flush 与回收。

---

## 优化点清单（按预期收益从高到低）

### M-1. 每次 trap 与每次 PTE 变更均全局 TLB flush 【高】

- **位置**：
  - `os/components/wateros-platform/platform-arch/arch-impl/impl-riscv64/asm/trap.asm:88-91,264-266,294-296`
  - `os/components/wateros-platform/platform-arch/arch-impl/impl-loongarch64/asm/trap.S:161-165`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:646,694,708,763,795`
  - `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs:589,619,631,683,714`
- **当前实现/复杂度**：RV 用户 trap 入口/返回各一次 `sfence.vma x0,x0`，MM 层 `flush_address_space_translations()` 同样全局；LA 回用户 `invtlb 0,$zero,$zero` 全局。单次 syscall/缺页/tick ≥2 次全局失效，COW/懒加载/munmap 改 PTE 后再加 1 次。
- **问题**：全局 flush 使 TLB 几乎无法复用；RV 已分配 ASID 但 flush 策略使其完全失效。
- **改进方案**：trap 同 ASID/同根表跳过 flush；MM 单页更新用 `sfence.vma va` 定向，批量 unmap 合并为一次 flush；多核引入 IPI shootdown + ASID generation bitmap。
- **预期收益**：高，影响所有用户态路径。
- **架构差异**：RV 可用 `sfence.vma asid`；LA 无 ASID（`satp_value` 仅 `root*PAGE_SIZE`，见 `impl-loongarch64/.../pagetable.rs:840`），需 PGDL+VPN 定向无效化或硬件 ASID。
- **风险/依赖**：与 trampoline/satp 切换设计强耦合；错误 flush → UAF/错页。

### M-2. RV ASID 循环复用但无 generation / shootdown 【高（selective flush 前置）】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:158-193,306,650`
- **当前实现/复杂度**：`NEXT_USER_ASID.fetch_add(1) & 0xFFFF`，跳过 0/1，O(1) 分配，65535 后循环复用。
- **问题**：配合全局 flush（M-1）ASID 无收益；若改 selective flush 而不维护 generation，复用 ASID 时旧 TLB 条目可能命中错误映射。
- **改进方案**：ASID → `(generation, index)` 或 per-ASID 版本号；复用前对该 ASID shootdown；进程退出归还 ASID 到空闲池并记录 generation。
- **预期收益**：高（多核 + selective flush 后）；单核+全 flush 下为 correctness 前置。
- **架构差异**：仅 RV；LA 当前无 ASID。
- **风险/依赖**：依赖 M-1 flush 改造；与 `fork_user_aspace` / `drop_user_aspace` 生命周期绑定。

### M-3. fork 完整复制页表树 + 每中间节点分配并 512 循环清零 【高】

- **位置**：
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:629-664,847-902,222-230`
  - `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs:572-605,781-834`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/lib.rs:97-111`
- **当前实现/复杂度**：数据页走 COW（共享 PPN + `frame_inc_ref` + 清 W 位），O(已映射用户页)；页表对每个非空中间节点 `alloc_table_frame_zeroed()` + 递归 `fork_table`，O(页表节点数)；每表帧 `frame_alloc` + 512 项循环清零。
- **问题**：fork 时 CPU/内存带宽消耗在页表复制；大地址空间 fork 延迟高；每次 `frame_inc_ref`/`alloc_table_frame_zeroed` 独立持帧锁（见 M-8）。
- **改进方案**：延迟 fork 页表（子进程先共享父页表页，写时复制 PTE 页）；表帧清零改 `write_bytes` 整页一次；批量 inc_ref 减少锁次数。
- **预期收益**：高，fork/clone 压测（LTP）直接受益。
- **架构差异**：两架构逻辑对称；LA 无 ASID，子进程 PGDL 必变，TLB 压力更大。
- **风险/依赖**：页表 COW 实现复杂，与 `destroy_table` 回收 ownership 需一致。

### M-4. 进程退出 `destroy_table` 逐表固定 512 扫描 + 每帧独立 dealloc/加锁 【高】

- **位置**：
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:666-680,804-837`
  - `os/components/wateros-mm/mm-impl/impl-loongarch64/src/pagetable.rs:718-771`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/lib.rs:128-141`
- **当前实现/复杂度**：递归 DFS，每层固定遍历 512 PTE（即使稀疏），叶子/中间表逐帧 `frame_dealloc_result`；O(页表节点数×512) 扫描 + O(映射页+表帧) 次 `with_frame_allocator`；`shared_anon_vmas` 标记页跳过 dealloc。
- **问题**：exit/reap 路径慢；大量小映射进程 exit 时帧回收延迟；共享匿名页永不回池（见 M-18）；每次 dealloc 触发 M-8 的 O(n) `contains`。
- **改进方案**：页表节点引用计数 + 空子树 lazy collapse（与 M-5 统一）；批量 dealloc（局部数组 + 单次持锁）或 per-CPU 延迟回收队列。
- **预期收益**：高，进程 churn、LTP exit/wait。
- **架构差异**：逻辑相同；leaf 判定不同（RV `is_leaf_at_level` vs LA `flags.is_leaf()`）。
- **风险/依赖**：与 MAP_SHARED/COW refcount 语义需先闭环。

### M-5. munmap/brk 收缩不回收中间页表帧（慢泄漏 + 地址空间膨胀）【高】

- **位置**：
  - `os/components/wateros-mm/mm-api/api-v0/src/address_space.rs:119-139`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:922-934`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs:72-87,435`
- **当前实现/复杂度**：unmap O(页数×walk)，仅 `pte.clear()` + dealloc 数据帧；中间表帧永久保留。
- **问题**：重复 mmap/munmap 抖动 → used_frames 单调上升；fork 复制的页表树更大；destroy 需遍历更多空节点。
- **改进方案**：post-unmap 向上 walk 检测全空子表 → dealloc 中间帧并 collapse；或 VMA 粒度按需建表。
- **预期收益**：高，长测/LTP mmap 压力。
- **架构差异**：无本质差异。
- **风险/依赖**：勿误删与内核恒等映射共存的中间节点；并发 unmap 需锁页表。

### M-6. 匿名/brk/栈路径每页强制 `write_bytes` 清零 4 KiB，懒 fault 双清零 【高】

- **位置**：
  - `os/components/wateros-mm/mm-impl/common/src/lib.rs:217-224,299-311`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:212-218,747-748`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs:49-71,113-117,142-143`
- **当前实现/复杂度**：`map_zeroed_page_with_alloc` = alloc + `zero_phys_page`(4096B) + map；brk 扩展饥渴逐页映射；私有匿名 mmap 已 lazy（`ZeroAnonLoader`），但 brk/栈/共享 anon 仍 eager；懒 fault 路径 `dst.fill(0)` 后再 `load_page` 双清零。
- **问题**：大 brk 扩展、MAP_SHARED anon、栈 touch 立即消耗内存带宽；懒 fault 重复清零。
- **改进方案**：brk 改 lazy zero page（仅扩 VMA/brk 指针，fault 再分配，见 M-19）；全零帧 singleton + COW；懒 fault 去掉 `fill(0)`。
- **预期收益**：高，brk/mmap 扩展、缺页热路径。
- **架构差异**：无。
- **风险/依赖**：安全要求新页无旧数据，须保证零页。

### M-7. `find_free_mmap_base` 线性探测 + 每候选页 walk 【中高】

- **位置**：`os/components/wateros-mm/mm-impl/common/src/lib.rs:236-275`、`impl-sv39/src/pagetable.rs:417-471`
- **当前实现/复杂度**：从 cursor 逐页探测，最多 `2^20` 页；每候选对 `n_pages` 调 `translate_addr`（3 级 walk）→ O(skipped×n_pages×3)；considering_vmas 再线性扫 `lazy_file_vmas`/`shared_anon_vmas`。
- **问题**：地址空间碎片多、mmap 频繁时 placement 变慢；mremap grow 冲突触发 relocate 加倍。
- **改进方案**：VMA 区间树/有序链表维护空闲区，first/best-fit O(log n)；probe 用页表 bitmap 或 VMA 重叠检测代替逐页 translate。
- **预期收益**：中高，多 mmap 工作负载。
- **架构差异**：无。
- **风险/依赖**：与 lazy VMA 元数据一致性；MAP_FIXED 单独处理。

### M-8. 帧分配器 `dealloc_frame` 中 `recycled.contains` O(n) + 全局关中断锁 【中】

- **位置**：`os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs:124-187,267-273`
- **当前实现/复杂度**：分配 O(1)；回收 refcount>1 仅递减，否则 push recycled，校验 `recycled.contains(&frame)` O(|recycled|)。**已抽查确认**：`alloc_frame`/`dealloc_frame` 已维护 `allocated[idx]` 位图，`!self.allocated[idx]` 即可 O(1) 判重，`contains` 校验冗余。所有 API 经 `FrameAllocatorInterruptGuard` + `RefCell` 关中断互斥。
- **问题**：高 churn 回收时临界区随 recycled 栈深度线性增长；COW/fork/destroy 大量 dealloc 放大延迟。
- **改进方案**：去掉 `recycled.contains`，直接用 `allocated[]` 位图判重；批量 dealloc API；元数据迁静态区（见 M-12）。
- **预期收益**：中，帧压力、fork/exit 密集时明显。
- **架构差异**：无。
- **风险/依赖**：持借期间 `Vec` 增长可能嵌套堆分配（SFA-2）；多核需原子锁。

### M-9. COW fault / `ensure_private_for_write` 多次独立帧锁 + 整页 memcpy + 全局 flush 【中】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:682-710,767-797`、`impl-loongarch64/src/pagetable.rs:607-633,687-716`、`user_heap_mmap.rs:466-484`
- **当前实现/复杂度**：写 fault `walk_find` → `frame_ref_count` → 可能 `frame_alloc` + `copy_nonoverlapping(4096)` + `frame_dealloc` + PTE 更新 + 全局 flush；每次帧 API 独立 `with_frame_allocator`（3~4 次锁）；mprotect 写权限对区间逐页 `ensure_private_for_write`。
- **问题**：COW 写热点锁竞争 + 内存拷贝 + flush 三重开销；mprotect 大范围触发批量 COW。
- **改进方案**：合并 COW 分裂为单次持锁；flush 改单 VA；mprotect 延迟到写 fault。
- **预期收益**：中，fork 后写密集、CoW mprotect。
- **架构差异**：PTE COW 位布局不同，流程一致。
- **风险/依赖**：refcount 与 MAP_SHARED 语义（M-18）。

### M-10. 页表 walk 每次 map/unmap/translate 三级 chase，无 PTE 缓存/大页 【中】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:577-621,950-965`、syscall 拷贝 `user_access.rs:87-115`
- **当前实现/复杂度**：`walk_create`/`walk_find` 固定 3 次内存访问；`translate_addr` O(3)；大缓冲 copy 每 4K 一次 walk。
- **问题**：syscall read/write、find_free_mmap、region_is_mapped 重复 walk；无 huge page 减页表深度与 TLB 压力。
- **改进方案**：软件 PTE 缓存（最近 VPN→PPN）；批量 map/unmap 按 contiguous run 合并；长期引入 Sv39 megapage。
- **预期收益**：中，syscall 大块拷贝、频繁 translate。
- **架构差异**：LA 三级结构相同。
- **风险/依赖**：megapage 与 COW/lazy 交互复杂。

### M-11. `map_zeroed_range_with_alloc` / brk 扩展部分失败无回滚 【中】

- **位置**：`os/components/wateros-mm/mm-impl/common/src/lib.rs:277-296`、`impl-sv39/src/user_heap_mmap.rs:49-71`
- **当前实现/复杂度**：循环 `map_zeroed_page_with_alloc`，任一失败直接 Err，已映射前缀不回滚（对比 `map_range_from_loader:357-365` 有回滚）。
- **问题**：OOM 后 VA/PTE/帧账本不一致；后续 mmap/translate 见幽灵映射；浪费帧至进程 exit。
- **改进方案**：失败时 `unmap_range_with_alloc` 回滚前缀；或 transactional batch map。
- **预期收益**：中，帧池边界行为 + 间接减 exit 清理量。
- **架构差异**：无。

### M-12. 帧分配器元数据常驻内核堆（~9 B/帧）【中低】

- **位置**：`os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs:60-97`
- **当前实现/复杂度**：init 时 `allocated: Vec<bool>` + `ref_counts: Vec<usize>` resize 到帧数；1GiB RAM ≈ 256K 帧 → ~2.25 MiB 堆常驻，永不收缩；resize 在持帧锁+关中断内触发堆分配。
- **问题**：压缩可用堆预算；引导期锁内嵌套堆分配（SFA-2）。
- **改进方案**：静态 BSS 位图（编译期最大 RAM 上限）；refcount 压缩到 16bit；元数据独立静态池。
- **预期收益**：中低，堆紧张/大内存机器。
- **架构差异**：无。
- **风险/依赖**：引导顺序须先 `heap_allocator::init`。

### M-13. 内核堆 `linked_list_allocator` + 关中断 + 非 O(1) 分配 【中低】

- **位置**：`os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs:23-66,92-129`
- **当前实现/复杂度**：`LockedHeap`（链表空闲块）+ `spin::Mutex` + 关中断守卫；分配/释放典型 O(n) 扫描；128 MiB 堆，OOM panic；已有递归检测与 90% 高水位 warn。
- **问题**：TCB/VMA/FD/`Box<dyn Loader>` 频繁 alloc 时碎片化 + 关中断延长临界区；与帧分配器 Vec 增长交叉。
- **改进方案**：slab/buddy（按 size class）；多核 per-CPU cache；syscall 路径 fallible alloc；缩短关中断仅包 Mutex 内部。
- **预期收益**：中低，元数据密集、长测堆碎片时。
- **架构差异**：无。
- **风险/依赖**：嵌套 alloc 死锁（ISH-1）；审计日志路径。

### M-14. lazy VMA 管理 Vec 线性扫描 + munmap 分裂 O(n) 且 `duplicate_box` 【中低】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:473-529,716-728`、`user_heap_mmap.rs:436-439`
- **当前实现/复杂度**：缺页 `lazy_file_vmas.iter().position` O(#VMA)；`remove_lazy_file_vmas` drain 全表，重叠 split 并 `loader.duplicate_box()` 堆分配。
- **问题**：多 mmap 区进程 fault 慢；munmap 部分区间触发 loader 复制与 Vec 重建。
- **改进方案**：按起始地址排序 Vec 或 interval tree；VMA 存 `Arc<Loader>` 避免 split 复制；fork 共享 VMA 树。
- **预期收益**：中低，多映射进程。
- **架构差异**：无。

### M-15. 文件 mmap：MAP_PRIVATE 已可 lazy，但 MAP_SHARED/小文件仍 eager 整段映射 【中低】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs:201-251,347-401`、`common/src/lib.rs:313-335`
- **当前实现/复杂度**：`mmap_file`/`map_range_from_backing` 饥渴逐页 alloc+fill+map O(n_pages)；`mmap_file_lazy` 仅登记 VMA fault 再读。
- **问题**：大文件 eager mmap 瞬时耗尽帧池与 CPU；与私有 anon lazy 策略不一致。
- **改进方案**：默认 private file 走 lazy；shared 按需 pin；结合页缓存避免用户帧 vs 页缓存双重复制（与 `perf-fs-vfs.md` F-15 协同）。
- **预期收益**：中低，大文件 mmap。
- **架构差异**：无。
- **风险/依赖**：MAP_SHARED 与 M-18 refcount 联动。

### M-16. mremap grow 冲突 relocate：新区清零 + 逐字节 `copy_mapped_bytes` 【中低】

- **位置**：`os/components/wateros-mm/mm-impl/common/src/lib.rs:411-465,537-562`
- **当前实现/复杂度**：relocate = `find_free_mmap_base` + `map_zeroed_range_with_alloc`（新页全清零）+ `copy_mapped_bytes`（跨页 walk+memcpy）+ unmap old，O(old+new) 内存流量。
- **问题**：grow 遇冲突比 Linux PTE move 重得多；新扩展区清零浪费。
- **改进方案**：能 in-place 扩则仅 map 增量页；relocate 时 move PTE/PPN 而非 memcpy 物理内容。
- **预期收益**：中低，mremap 密集应用。
- **架构差异**：无。
- **风险/依赖**：COW 页 move 需 break COW。

### M-17. `alloc_table_frame_zeroed` 512 次 PTE 循环写 vs 整页清零 【低中】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:222-230`、`impl-loongarch64/src/pagetable.rs:205-213`
- **当前实现/复杂度**：分配一帧后 `for e in tbl.iter_mut() { *e = zero() }` 512 次写；fork/walk_create 频繁调用。
- **问题**：比单次 `write_bytes(pa,0,4096)` 慢；fork 大量表帧分配放大（叠加 M-3）。
- **改进方案**：统一 `zero_phys_page(ppn)` 或 `MaybeUninit` 批量写。
- **预期收益**：低中，fork/map 频繁时累积明显。
- **架构差异**：无。

### M-18. MAP_SHARED 匿名 fork 不 inc_ref，munmap/destroy 语义不一致（回收隐患）【性能低 / 正确性 P0】

- **位置**：
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/pagetable.rs:822-827,871-876`
  - `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs:179-182`
  - `os/components/wateros-mm/mm-api/api-v0/src/address_space.rs:81-92`
  - LA 同逻辑 `impl-loongarch64/.../pagetable.rs:757-761,804-806`
- **当前实现/复杂度**：共享 anon eager 映射 + `register_shared_anon_vma`；fork 对 shared 不 `frame_inc_ref`；munmap 无条件 dealloc；destroy 对 shared 页跳过 dealloc → 永久占帧。
- **问题**：UAF 风险 + 帧池只减不增；性能表现为「可用内存虚低」，长测 OOM。
- **改进方案**：shared 页统一 refcount；destroy/munmap 按 ref 释放；或 MAP_SHARED 走 shmem 帧池。
- **预期收益**：性能低 / 正确性 P0；修复后帧回收率恢复。
- **架构差异**：RV/LA 同逻辑。
- **风险/依赖**：与 T-PF-02/03 审计任务绑定。

### M-19. brk 仍饥渴映射，与私有 anon mmap lazy 策略分裂 【中】

- **位置**：`os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs:49-71,121-144`
- **当前实现/复杂度**：brk 扩展逐 VPN `map_zeroed_page_with_alloc`；`handle_brk_page_fault` 仅作补页；注释 `:5-6` 称 eager，与 anon lazy 不一致。
- **问题**：glibc 大 brk 扩展仍瞬时吃帧；策略不统一使 brk 路径成为帧池热点。
- **改进方案**：brk 扩展只更新 `user_brk_current_end`，映射全靠 fault（与 anon lazy 统一）。
- **预期收益**：中，传统 brk 分配器工作负载。
- **架构差异**：无。
- **风险/依赖**：保证 `brk(expand)` 与 Linux 可见内存语义一致。

### M-20. `handle_page_fault` 链路过长 + COW/lazy 分两次 trap 尝试 【中】

- **位置**：`os/src/trap_handler.rs:187-216`、`os/components/wateros-mm/src/lib.rs:99-128`、`user_heap_mmap.rs:403-417`
- **当前实现/复杂度**：store fault 先 `handle_cow_fault` 再 lazy/stack/brk；每次 fault `with_user_aspace_mut` + 帧锁 + 可能 `fence_user_ptes`，成功后全局 flush。
- **问题**：缺页链路长；COW 与 lazy 分两次 trap 尝试；每次成功 fault 全局 flush。
- **改进方案**：合并 fault 解码一次分发；减少 trap 层与 MM 层往返；成功用 VA 定向 flush（与 M-1、`perf-hotpath.md` H-15 协同）。
- **预期收益**：中，懒加载/COW 密集。
- **架构差异**：trap 原因编码不同，分发逻辑可共享。

---

## 架构差异速览

| 维度 | RISC-V (Sv39) | LoongArch64 |
|------|---------------|-------------|
| 地址空间 token | `satp` MODE+ASID+PPN | PGDL=root×PAGE_SIZE，无 ASID |
| TLB flush | `sfence.vma` 全局 | `invtlb 0,$zero,$zero` 全局 |
| trap 进内核 | asm 切 kernel satp + flush | Rust trap_handler 比较 token 后 activate |
| 页表层级 | 3×512，4K leaf | 同构 |
| COW/fork/destroy | 同逻辑 | 同逻辑 |

## 与审计文档的漂移修正（已部分实现，记录避免误判）

- clone fork 失败已用 `CloneSetupGuard`/`drop_user_aspace` 回滚（`sys/clone.rs:237-248`）。
- brk OOM 已传播 `MmError`（`sys/brk.rs:19-25`）。
- 内核堆已增 `heap_mem_stats()` 与高水位 warn。
- 私有 anon mmap 已实现 lazy（`ZeroAnonLoader`），但 `user_heap_mmap.rs:5-6` 注释仍写「饥渴映射」已过时。

## 落地优先级建议

1. M-1 TLB flush 策略（trap + MM 分层）
2. M-5 页表中间帧 collapse + M-4 destroy 批量化
3. M-3 fork 页表延迟复制 + M-17 表帧清零
4. M-6 brk/懒 fault 零页策略 + M-19 brk lazy 统一
5. M-8 帧 dealloc 去 O(n) + M-7 VMA 索引
6. M-18 MAP_SHARED refcount（正确性前置）

## 后续维护入口

- 改 TLB/ASID/页表：同步两架构 `arch-impl`、`mm-impl`、本文档与 `perf-hotpath.md`。
- 改帧分配器/堆：同步 `docs/audits/resources/physical-frames.md`、`kernel-heap.md`、`docs/architecture/snapshot.md`。
- 改 fork/COW/回收语义：同步 `docs/audits/resource-inventory.md` 生命周期钩子表与 `perf-lock-resource.md`。
