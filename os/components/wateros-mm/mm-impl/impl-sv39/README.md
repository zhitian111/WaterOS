# RISC-V Sv39 MM 实现离线开发手册

[MM 总览](../../README.md) · [公共实现](../common/README.md) · [MM API](../../mm-api/api-v0/README.md) · [RISC-V 架构手册](../../../wateros-platform/platform-arch/arch-impl/impl-riscv64/README.md)

本 crate 实现 WaterOS 的 RISC-V64 Sv39 页表、用户地址空间、ASID、COW、ELF/脚本
装载、用户拷贝及 `brk/mmap`。公共 VMA、lazy fault、只读缓存和 `mremap` 数据复制来自
`impl-common`；PTE 位、`satp`、页表树生命周期和 shootdown 在本 crate。

本文以当前源码为准。标为“当前限制/缺陷”的内容要保留在比赛排障清单中，不能按
Linux 完整语义理解。

## 1. 文件地图

| 文件 | 职责 |
| --- | --- |
| `pagetable.rs` | Sv39 PTE、三级 walk、VMA 容器、COW fork、页表销毁和映射快照 |
| `asid.rs` | ASIDLEN 探测结果接入、用户 ASID 位图、回收 |
| `kernel_global.rs` | 帧池初始化、RAM/MMIO 恒等映射、内核 `satp` 与生命周期 hook |
| `user_aspace.rs` | raw handle、地址空间互斥、CPU 缓存位图、TLB shootdown |
| `user_heap_mmap.rs` | `HeapBrk/MmapOps`、stack/brk/lazy fault、madvise |
| `user_access.rs` | 跨页用户拷贝、原子 u32、futex 物理身份、调试 probe |
| `kernel_elf.rs` | ELF64-RISC-V、lazy 段 loader、动态解释器、栈和 signal trampoline |
| `kernel_executable.rs` | ELF/shebang 判别和递归解释器 argv 重写 |
| `lib.rs` | 聚合导出、fork/COW/drop hook、自检 |

## 2. Sv39 地址与 PTE

### 2.1 页表几何

```text
页大小             4 KiB
每级条目           512（9 bit）
层级               3：VPN[2] -> VPN[1] -> VPN[0]
覆盖                512 GiB 虚拟空间
WaterOS 用户上界    0x0000_0040_0000_0000（256 GiB，不含）
satp MODE           8（Sv39）
satp ASID           bit 59..44
satp root PPN       bit 43..0
```

`vpn_indexes(vpn)` 返回低到高 `[vpn0,vpn1,vpn2]`；walk 按 2、1、0 下降。当前所有新
映射都是 level 0 的 4 KiB 页，不创建 2 MiB/1 GiB superpage；`translate_addr` 遇到
高层叶返回 `Unsupported`。

软件 walker 只提取 27 位 VPN 索引，并不自行检查 Sv39 canonical sign-extension。
普通 mmap 范围由 `USER_VA_LIMIT` 限制，但 `user_access` 的任意 syscall 指针会直接进入
walker。当前应补一层统一的 lower-canonical 用户地址检查，避免非 canonical 地址在
软件 walk 中别名到高半区索引；即使当前高半区 PTE 无 `U`，也不应依赖这一偶然条件。

### 2.2 PTE 位

| 位 | 名称 | 当前用途 |
| ---: | --- | --- |
| 0 | V | 有效；中间项只置 V |
| 1/2/3 | R/W/X | 硬件叶权限 |
| 4 | U | 用户可访问；也是销毁时识别用户叶的重要条件 |
| 6/7 | A/D | 映射时预置，避免依赖硬件/软件访问位 fault |
| 8 | COW | RSW 软件位：当前叶需要写时复制 |
| 9 | COW_WAS_WRITABLE | RSW 软件位：fork 前曾可写 |
| 10..53 | PPN | 44 位物理页号 |

`from_perm` 始终置 `V|A|D`。`PROT_NONE` 因而编码成 level-0 `V|U`、无 R/W/X；硬件
访问会 fault，但软件的 `is_leaf_at_level(0)` 把它当作已占用的语义叶，确保 unmap、
destroy 和重复 map 正确处理。

修改 PTE 时必须保持 RISC-V 规则：`W=1,R=0` 是保留/非法组合。当前 `PagePerm` 到 PTE
没有主动把 W 推导成 R，调用者必须传合法权限。

### 2.3 物理访问假设与 unsafe

`table_mut(ppn)`、COW copy、用户 copy 和 ELF 填页都把 `ppn*4096` 直接作为内核指针。
这依赖 RAM 在内核页表中恒等映射。若改成高半核/direct-map offset，必须统一替换所有
这些位置。

`walk_find(&self)` 返回 `&'static mut Sv39Pte`，从共享借用制造可变引用在 Rust 类型层面
并不安全；当前依赖 `UserAddressSpaceCell::MultiprocessorSafeCell` 串行化。任何绕开
`with_user_aspace_mut*` 的访问都会破坏该隐含安全条件。长期应让 walker 返回 PTE 地址或
受 guard 生命周期约束的引用。

## 3. `Sv39AddressSpace` 数据结构

```text
root                    根页表 PPN
asid                    u16；0 保留内核/无硬件 ASID 退化
user_brk_start/current/max
mmap_anon_cursor        私有/共享匿名选址游标
mmap_file_cursor        文件/设备选址游标
mmap_base               first-fit arena 下界
user_stack_bottom/top   可按需补页的保留范围
lazy_file_vmas          LazyVmaSet：private anon、private/file lazy、ELF lazy
shared_anon_vmas        MAP_SHARED 匿名和共享文件物理页身份标记
shared_file_vmas        MAP_SHARED 文件 writeback loader
device_vmas             外部 PPN、权限和 lease
```

共享文件映射会同时出现在 `shared_anon_vmas`（保证 fork/解除时按共享引用处理）和
`shared_file_vmas`（写回）。设备页只在 `device_vmas`，不能交给 frame allocator。

`unsafe impl Send/Sync` 的理由同样是“只能经 cell 锁访问”；`DemandPageLoader` 自身没有
要求 `Send`，所以把地址空间裸引用跨线程传递是未定义行为风险。

## 4. 页表 walk、map 与 unmap

### 4.1 `walk_create`

```text
root(level 2)
  -> 无效：分配清零子表，父 PTE=V
  -> 高层已有 R/W/X 叶：AlreadyMapped
  -> level 1 同样处理
  -> level 0 返回 PTE 槽
```

中间表分配成功后，若更低层稍后分配失败，已挂入的空中间表不会回滚。unmap 单页也不
剪枝空表；这些页表帧只在地址空间销毁时释放。稀疏映射反复创建/删除可能保留较多中间
表，这是排查“RSS 已降但物理帧未完全回基线”的一个来源。

### 4.2 `AddressSpaceOps`

- `map_page_to_ppn`：创建 walk，叶槽已 `V` 则 `AlreadyMapped`，不接管失败时的 PPN。
- `unmap_page_to_ppn`：只清 PTE并返回旧 PPN；调用者决定是否释放。
- `protect_page`：保留 PPN、用 `from_perm` 重写 flags，因此会清掉 COW 软件位。
- `translate_addr`：只支持 4 KiB 叶，返回包含页内 offset 的完整 PA。
- `leaf_page_perm`：把 COW PTE看到的权限按当前硬件位返回，因此 W 已被清除。
- trait 的 `fork()` 当前返回 `Unsupported`；正式 fork 走可变的 `fork_cow()`。

这些低层函数不自动 TLB flush。syscall 路径必须包在 `with_user_aspace_mut_and_flush*`
中；fault 专用路径可用页级 flush 包装。

## 5. 内核页表启动链

```text
kernel_mm::init(dtb_pa, ram_end)
  -> 从 linker kernel_end 算 frame pool 起点
  -> 从 DTB total_size 算保留页，初始化全局 frame allocator
  -> Sv39AddressSpace::new_kernel（ASID 0）
  -> 4 KiB 恒等映射 [0x80000000, ram_end) 为 RWX
  -> 恒等映射 QEMU virt MMIO 和 Goldfish RTC 为 RW
  -> 建立一个非恒等 probe VA -> pool 内 probe PPN
  -> 设置 trap kernel satp，安装 satp 并验证翻译/读写
  -> 探测 ASIDLEN，初始化 ASID allocator
  -> BootOnceCell 保存内核地址空间，发布 kernel_satp
  -> 注册 drop、CPU active/inactive、mapping snapshot hooks
```

DTB 地址不再被误当作 RAM 末端，只把实际 blob 页排除出帧池。8 GiB RAM 使用 4 KiB
叶映射会消耗约两百万个叶 PTE，页表本身也占用显著物理帧；这不是内核 heap，却会影响
`MemFree` 基线。

当前限制：

- RAM 整段映射为 RWX，没有内核 text/rodata/data W^X 分离。
- probe PPN 直接选择 `start_ppn+16`，没有从 allocator 取出或永久 reserve；probe 后该
  PPN 仍可能被正常分配。残留 probe VA 当前不再使用，但这不是严格所有权模型。
- `map_anon_range_user` 分配后不清零，可能暴露旧帧内容；若 map 返回
  `AlreadyMapped`，刚分配 PPN 也未释放。该接口只应视为受信 bring-up 兼容路径。
- `map_identity_range_user/ensure_user_execute_for_kernel_va/map_anon_range_user` 自身不做
  TLB flush，调用者必须保证尚未运行或随后统一 fence。
- 内核地址空间永久保留，符合生命周期预期；不能在运行期重做 `init`。

## 6. ASID 与 TLB shootdown

### 6.1 ASID 分配

启动时 `initialize_address_space_ids()` 探测 ASIDLEN，软件最多使用 16 位。ASID 0 保留
内核：

- ASIDLEN=0 时每个用户地址空间都得到 0，架构切换路径必须全量 `sfence.vma`；
- 有 ASID 时位图从 1 线性扫描到 `(1<<bits)-1`；耗尽映射为 `OutOfMemory`；
- 回收前必须让所有可能缓存该 ASID 的 hart 完成失效。

`initialize()` 必须在任何用户地址空间分配前调用。运行中缩小 limit 不会迁移已分配
编号，属于未支持操作。

### 6.2 handle 和 CPU 追踪

`into_handle` 把 `Box<UserAddressSpaceCell>` 转成 `usize` 裸地址。cell 包含：

```text
inner       MultiprocessorSafeCell<Sv39AddressSpace>
dropped     AtomicBool，阻止重复 destroy/新访问
tlb_cpus    AtomicU64，只增不减的“曾缓存此 ASID”CPU 位图
token       创建时固定的 satp
```

`mark_active` 首次在某 CPU 使用非零 ASID 时先做该地址空间 local flush，再把 CPU 留在
永久位图中；`mark_inactive` 故意不清位，因为换 satp 后 TLB 仍可能保留旧 ASID 项。

**重要常驻泄漏设计**：`destroy(handle)` 只销毁页表/VMA/ASID，不执行
`Box::from_raw(handle)`。cell 作为 tombstone 永久留在内核 heap，使 stale handle 能读取
`dropped` 而不 UAF。每次 fork/exec/exit 都留下一个小 cell，`forkheavy` 长时间运行会
线性增长内核 heap；这与“进程已退出但 heap used 持续上升”直接相关。正确修复需要带
generation/refcount/epoch 的句柄表，等所有引用退出后回收 slot，而不是永久泄漏 Box。

任意非零 `usize` 并不真的可验证：`cell()` 会直接解引用，安全依赖 handle 只来自内核
内部。不要让用户 ABI 或可损坏对象直接提供此值。

### 6.3 shootdown 协议

```text
修改者 local flush
  -> tlb_cpus & online_mask - current_cpu
  -> 全局 TLB_SHOOTDOWN_LOCK 串行事务
  -> 优先 platform::flush_tlb_remote(targets)
  -> Unsupported/失败时：分配 sequence
       -> TLB_PENDING[cpu]=sequence
       -> send TlbShootdown IPI
       -> 等 TLB_COMPLETED[cpu] >= sequence，最多 10,000,000 spins
远端 handle_tlb_shootdown_ipi
  -> local All flush
  -> completed=sequence
```

等待全局锁时主动调用 `handle_tlb_shootdown_ipi`，避免“关中断的 B 等 A 锁、A 等 B IPI”
死锁。pending 每 CPU 只有一个槽，因此所有请求必须全局串行。

包装函数选择：

| 包装 | 何时 flush | 用途 |
| --- | --- | --- |
| `with_user_aspace_mut` | 不 flush | 纯查询或内部自行处理 |
| `..._and_flush` | 成功/失败均 local All + remote | fork、复杂多步修改 |
| `..._flush_if_changed` | changed 或错误时 All + remote | mprotect/munmap |
| `..._and_page_flush` | 成功后总 local page；changed 才 remote | COW fault |

当前限制：

- 普通修改路径忽略 shootdown 的 `false`，IPI 发送失败/超时后 syscall 仍可能成功，远端
  保留 stale PTE；只有 destroy 会因此退休 ASID。这是 SMP 正确性缺口。
- 软件 IPI flush 总是 `All`，没有携带 token/page，代价较大。
- `and_page_flush` 的闭包若返回错误，会在 `?` 处直接返回，没有执行注释所说的
  unconditional local flush；fault 闭包当前应避免“先改 PTE 再返回 Err”。
- sequence 使用 `usize` 并按普通大小比较，理论 wrap 后顺序关系失效。

## 7. fork、COW 和销毁

### 7.1 fork 调用链

```text
sys_clone/fork
  -> kernel_mm_impl::fork_user_aspace(parent_handle)
  -> with_user_aspace_mut_and_flush(parent)
  -> parent.fork_cow()
       -> fallible duplicate lazy/shared-file loaders
       -> 分配 child ASID 和 root
       -> fork_table(parent_root, child_root)
       -> 父 PTE 被改 COW，所以 local fence
  -> child into_handle，task 保存 child satp/handle
```

`fork_table` 对叶子的策略：

| 叶类型 | PPN 引用 | flags |
| --- | --- | --- |
| 无 U 的内核 trampoline | 不增 | 父子直接共享 |
| device VMA | 不增通用 frame ref | flags 原样，lease 在 VMA clone |
| shared anonymous | 增 ref | 可写仍可写，不 COW |
| 普通只读/private cache | 增 ref | flags 原样 |
| 普通可写 private | 增 ref | 父子清 W，置 COW + WAS_WRITABLE |

当前重大缺口：`fork_table` 只接收 `shared_anon_vmas` 和 `device_vmas`，不接收
`shared_file_vmas`。共享文件映射创建时目前也登记到 shared-anon 列表，所以正常路径可
借此避开 COW；若未来重构移除这份双重登记，MAP_SHARED writable 会错误进入 COW，父子
物理页分裂且退出写回互相覆盖。修改这两张表时必须同时回归。

fork 失败时子树递归销毁并归还 ASID，但父页中已设置的 COW 不回滚。子引用释放后这些
页可能 refcount=1，父首次写会原地恢复 W；语义可继续运行，但失败 fork 会留下额外 fault。

### 7.2 COW fault

```text
store page fault
  -> handle_cow_fault(handle, addr)
  -> with_user_aspace_mut_and_page_flush
  -> handle_cow_page(vpn)
       非 COW/WAS_WRITABLE -> false
       refcount <= 1 -> 原 PPN 恢复 W/A/D，清软件位
       refcount > 1 -> alloc -> copy 4KiB -> dealloc old ref -> PTE 指向新 PPN
  -> local page flush；实际修改才 remote shootdown
```

若另一个 CPU 已完成 COW，本 CPU可能仍因 stale 只读 TLB trap；包装层看到当前 PTE 已
可写，会把它视为 handled 并仍做 local page flush。

错误边界：新 PPN 分配后若 `frame_dealloc_result(old_ppn)` 失败，新页没有释放且 PTE 尚未
切换。`ensure_private_for_write` 有相同顺序。更稳妥的事务要在不会失败的提交点切 PTE，
再用可诊断但不破坏映射的方式释放旧引用。

### 7.3 destroy

```text
drop_user_aspace -> user_aspace::destroy
  -> dropped 原子置位，重复调用直接返回
  -> 取走 tlb_cpus
  -> exclusive_access -> destroy_and_take_asid
       -> sync 全部 shared_file_vmas；失败只 warn
       -> destroy_table：用户非设备叶各减一次 ref，内核叶不减
       -> 释放所有页表帧
       -> drop VMA loader/lease Vec
  -> local AddressSpace flush + remote shootdown
  -> 成功才 release ASID；失败则永久退休编号
  -> cell tombstone 不释放
```

销毁写回失败无法返回给已退出进程，只记录一条聚合 warning；dirty 页随后仍会被释放。
排障时应在 `sync_shared_file_vmas` 打印 mount/node identity、VMA offset 和失败页，否则
仅凭 destroy warning 无法定位文件。

## 8. brk、mmap 与 VMA

### 8.1 brk

ELF 完成后：`brk_start=current=ceil(image_end)`，`brk_max=mmap_base`。增长时当前实现 eager
分配新增零页，并检查 stack、kernel window、lazy VMA；中途失败不完整回滚。收缩解除
尾页并释放。无论是否跨页都调用全地址空间 fence。

另有 `handle_brk_page_fault` 可为范围内缺失页补零页，主要处理被 madvise 丢弃或特殊
路径形成的洞；execute fault 被拒绝。

### 8.2 mmap 类型

| 请求 | 建立方式 | VMA/所有权 |
| --- | --- | --- |
| private anonymous | lazy | `LazyFileVma + Anonymous`，fault 才分配零页 |
| shared anonymous | eager | 全量零页 + `SharedAnonVma` |
| legacy file backing slice | eager | 普通 owned 页，无长期 loader |
| private/lazy file | lazy | `LazyFileVma + File loader` |
| shared file | eager | `SharedAnonVma + SharedFileVma`，munmap/destroy 写回 |
| device | eager 外部 PPN | `DeviceVma + lease`，只允许 SHARED、不可 X |

非固定 hint 当前不作为 hint 使用：有 `Some(addr)` 但没有 FIXED/FIXED_NOREPLACE 的多数
路径直接 `InvalidAddress`。`MAP_FIXED` 会先覆盖目标；共享文件目标先写回。操作失败后
旧目标通常无法恢复。`FIXED_NOREPLACE` 目前只在匿名路径明确支持，并返回通用
`InvalidAddress` 而非专门 AlreadyMapped errno。

选址从 `max(cursor,mmap_base,ceil(brk_current))` 开始，在最多 4 GiB 扫描窗口内避开 PTE、
stack、kernel trampoline、lazy 和 shared-anon。设备有驻留 PTE所以可见。新增纯元数据
VMA 类型必须加入 `find_free_mmap_base_considering_vmas`。

### 8.3 fault 顺序

```text
MmapOps::handle_page_fault
  -> stack fault（R/W；保留区内按需零页）
  -> brk fault（R/W；current break 内按需零页）
  -> common lazy file/private-anon fault
```

COW fault 不在这个函数内，由 trap 的写故障分类另行调用。stack 顶部实际预映射 16 页，
另额外映射 `ELF_STACK_TOP` 那一页；地址空间记录的 stack top 是
`ELF_STACK_TOP + PAGE_SIZE`。再上一页是 signal trampoline，不属于 stack VMA。

### 8.4 munmap/msync/mprotect/madvise

- `munmap`：先写回相交 shared file；成功后逐 PTE unmap，device 只断 PTE，其他页减 ref；
  再切分四类 VMA。后半元数据复制失败时 PTE 已撤销，操作非原子。
- `munmap_external`：所有叶都只断 PTE不减 ref，调用者必须先证明范围确为 SHM/外部页；
  误用会泄漏普通页。
- `msync`：每个相交 shared file VMA逐驻留页 `write_page`，然后 `flush`；非驻留页不写。
- `mprotect`：先改 lazy VMA，再逐驻留页；加 W 时非设备页先确保 private。中途 NotMapped
  可能留下前缀已修改，但 syscall 的 error 包装会保守全量 shootdown。
- `MADV_DONTNEED/FREE`：逐页用普通 allocator unmap，但保留 lazy VMA。若范围含 device
  或其他 external PPN，该 helper 没有检查 `non_owned_vma_contains`，存在错误回收风险；
  syscall 层当前用 `madvise_range_shared_or_file` 拒绝相应范围，新增调用者必须保留门禁。
- `prefault_all_current_user_ranges` 会把所有可访问 lazy VMA、brk 和完整 stack 变驻留，
  大栈会显著增加瞬时物理内存。

`sync_shared_file_vmas` 用 `mem::take` 临时移走 Vec，避免 loader 回调期间借用冲突，并在
成功/失败后恢复；但 `remove_shared_file_vmas` 与 common 的旧式 drain+duplicate 一样，
loader 复制失败会破坏 VMA 集合。

## 9. ELF、动态解释器和脚本

### 9.1 固定布局

```text
ET_EXEC bias             0
ET_DYN main bias         0x0040_0000
RISC-V interpreter base  0x7000_0000
preferred mmap base      0x1000_0000 或 image_end+64MiB 的较大值
stack top                0x7fff_a000
stack reserve            2 MiB
stack premap             16 页 + 顶外兼容页
signal trampoline        stack 顶外下一页；a7=139, ecall
```

只接受 ELF64 little-endian、`EM_RISCV=243`、`ET_EXEC/ET_DYN`。路径装载只读 64-byte
header 和 program headers，不整读大 busybox。`PT_INTERP` 被解析并映射到固定 base，最终
entry 指向解释器，`program_entry` 保留主程序入口。

用户页表中的内核窗口只包含 trap entry、user restore、kernel satp slot 和 return frame
所在页，且无 `U`。mmap/munmap 必须拒绝覆盖这些页。

### 9.2 段装载

启用 `elf-lazy-map` 时，未冲突的 PT_LOAD 页登记 `ElfPathSegmentLoader`，只读页可进入
全局 ELF cache；重叠段页合并权限。未启用时逐页 eager 分配并按连续片段读取，纯 BSS
整页可登记匿名 lazy VMA或从预清零池 prefault。入口页最终会验证文件字节与映射一致。

段前/文件尾/BSS 零填充、只读 cache key 和重复读取择优详见 common 手册。路径 loader
持有 `Arc<Mutex<Box<dyn VfsIoHandle>>>` 和内容 identity；不要在地址空间锁外另拿同一
handle 锁再触发 fault。

### 9.3 shebang

`load_program_from_path` 先读 256-byte prefix：ELF 直接装载；文本脚本解析 `#!`，按
Linux binfmt_script 风格重写 argv 后递归装载解释器。深度达到 API 的
`MAX_INTERPRETER_RECURSION` 返回 recursion error。`executable_path` 指向最终 ELF，argv
仍保留脚本语义。

## 10. 用户拷贝、原子和 futex

### 10.1 copy

`copy_from_user`/`copy_to_user_progress` 每页执行：软件翻译、必要时 demand fault、检查
`U` 和 R/W、按页内剩余长度复制。写入前处理 COW或把共享 ref 页私有化。

- copy-to 返回 `UserCopyProgress { completed, error }`，跨页第二页失败时保留已写前缀；
- copy-from 的返回类型是 `MmResult<usize>`，失败时 buffer 可能已有部分内容但调用者拿不
  到完成字节数；敏感 syscall 不应在错误后使用该 buffer；
- 空 slice 即使地址为 `usize::MAX` 也成功 0 字节，这是常见 ABI 约定；
- 当前 copy/CAS 的 COW PTE 替换在 `with_user_aspace_mut` 下完成，但只做本地 fence，未
  走 `request_tlb_shootdown`。多线程共享地址空间的远端 CPU可能继续读旧 PPN，是需要
  修复的 SMP 缺口。应让 copy 路径报告 PTE changed，并使用 page-flush wrapper。

### 10.2 atomic/futex

u32 地址必须 4-byte 对齐且不能跨页；load/CAS 使用 `SeqCst`。CAS 返回观察到的旧值，
无论比较是否成功。

`futex_mapping_identity_u32` 对 `shared_vma_contains` 返回真时用完整物理地址作为 shared
key，否则返回 Private。当前 `shared_vma_contains` 只查 `shared_anon_vmas`；共享文件
正常创建时也会登记该表，所以可以识别。若重构两表关系，必须显式加入
`shared_file_vmas`，否则不同进程的文件共享 futex 会被错误隔离。

## 11. 新增 MM syscall 实例

以新增 `mincore` 为例：

1. syscall ABI 检查 `addr` 页对齐、长度溢出、用户输出 vector 长度；
2. 取得当前 handle，调用 `with_user_aspace_mut`，因为只观察不改 PTE；
3. 对每页同时查询 `translate_addr` 与四类 VMA，resident 位只由 PTE决定；
4. 释放地址空间锁后用标准 user-copy API写 vector；不要持锁跨 VFS/user copy；
5. 任一输入页超出 lower canonical 用户区返回统一 errno。

若 syscall 会改 PTE，按结果选包装：

```rust
with_user_aspace_mut_and_flush_if_changed(handle, |aspace| {
    let changed = aspace.operation(...)?;
    Ok((result, changed))
})
```

不要在 `AddressSpace` 方法里自行 local flush 后又让 syscall 无条件 flush；更理想是方法
只返回精确变更范围，由一个包装层完成 local+remote。新增外部页映射还必须定义 PPN
所有者、fork 引用、munmap API、mprotect、madvise 和 Drop 行为。

## 12. 故障定位

### forkheavy 内核 heap 单调上升

先统计 `into_handle` 与 `destroy` 次数。当前每次 destroy 留下 tombstone cell，属于确定的
线性 heap 常驻；再分离 VMA loader/Vec 是否已在 `destroy_and_take_asid` 释放，以及全局
readonly cache 的物理帧常驻。不要只用进程 RSS 判断。

### 地址空间销毁写回失败

沿 `destroy -> sync_shared_file_vmas -> write_page/flush -> VFS handle` 追踪。确认文件仍有
稳定 handle/identity，mount generation 未在 dirty flush 前推进，且没有 VFS/地址空间
锁反转。当前 warning 信息不足时先加 identity/offset 日志。

### fork 后父子共享映射不一致

同时检查 `shared_anon_vmas`、`shared_file_vmas`、`fork_table` 的分类和 frame refcount。
共享 writable 必须 flags 原样；private writable 必须父子 COW。检查父 PTE 修改后的全
shootdown 是否完成。

### 用户 copy 后其他线程看到旧数据

检查 copy/CAS 是否触发 COW PPN 替换，以及远端 CPU 是否收到 shootdown。当前路径仅
local fence，是已知缺口；用双 CPU 同地址空间反复读写可复现。

### munmap 后物理帧未回收

区分页表中间页（到 destroy 才回收）、cache 自有引用、shared ref、device 外部所有权、
ASID/cell tombstone。用 frame refcount 跟踪 PPN，不要把所有残留都归因 VMA Vec。

### 重复 fault 或权限 fault

检查 PTE flags、COW 软件位、VMA perm、local `sfence.vma`、ASID token 与远端完成序号。
PROT_NONE 在软件中是 valid level-0 leaf，但硬件访问必然 fault，这是预期。

## 13. 自回归矩阵

- PTE：每个 R/W/X/U 组合、PROT_NONE、AlreadyMapped、unmap twice、非 canonical VA、
  中间表分配失败；
- fork：private R/O、private W、shared anon、shared file W、readonly cache、device、SHM，
  父/子各种退出顺序和 fork 中途 OOM；
- COW：ref=1 原地恢复、ref>1 copy、两个 CPU 同页同时 fault、copy-to/CAS 触发 COW；
- TLB：platform remote 成功、software IPI fallback、send failure、offline CPU、timeout、
  两个地址空间并发 shootdown；
- VMA：FIXED 覆盖、NOREPLACE、lazy 洞选址、共享写回失败、中部 munmap、跨 VMA
  mprotect、device 不回收；
- ELF：ET_EXEC/PIE、动态解释器、重叠 PT_LOAD、非页对齐、BSS、lazy/eager feature、
  shebang recursion；
- 用户访问：跨页部分 copy、权限失败、溢出、非 canonical、COW、shared futex identity；
- 压力：SMP=8 `forkheavy`、fork/exec/exit 循环，分别画 heap used、free frames、cache
  resident、tombstone 数和 ASID 可用数。

当前 `test_with_range/self_test` 只覆盖基础 map/protect/unmap、一个 lazy writable fault、
cache 基础 refcount 和 copy/COW；不覆盖真实 SMP shootdown、shared-file fork/writeback、
handle tombstone增长或失败注入。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
git diff --check
```

## 14. 修改检查清单

- [ ] 所有用户 VA 先验证 lower canonical 和 `USER_VA_LIMIT`。
- [ ] PTE 修改通过正确 local+remote flush 包装，错误路径也不会留下 stale TLB。
- [ ] fork 分类覆盖 private/shared-file/shared-anon/device/cache/SHM。
- [ ] 每次 `frame_inc_ref/alloc` 在所有失败分支都有配对释放。
- [ ] MAP_FIXED 的旧映射写回、目标销毁和新映射提交有明确事务边界。
- [ ] 新 VMA 同步接入选址、fault、fork、munmap、mprotect、mremap、madvise、destroy。
- [ ] 地址空间 handle 可安全回收，不以永久 tombstone 换取 stale-handle 安全。
- [ ] ELF 和用户 copy 不持 spin lock 跨 VFS I/O，物理直访假设未被破坏。
- [ ] RV/LA 共同语义同步，但 PTE/TLB 细节不机械复制。
