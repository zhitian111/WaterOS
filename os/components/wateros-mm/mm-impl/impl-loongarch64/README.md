# LoongArch64 MM 实现离线开发手册

[MM 总览](../../README.md) · [公共实现](../common/README.md) · [MM API](../../mm-api/api-v0/README.md) · [LoongArch64 架构手册](../../../wateros-platform/platform-arch/arch-impl/impl-loongarch64/README.md)

本 crate 实现 LoongArch64 三级页表、PGDL/ASID token、硬件 refill 所需目录、COW、
用户地址空间、ELF/脚本、用户拷贝和 `brk/mmap`。公共 VMA、只读页缓存、lazy fault 与
`mremap` 算法来自 `impl-common`。

它与 Sv39 暴露相同 MM API，但 PTE 格式、写保护、地址上界、用户页表内核窗口和 TLB
失效均不同。只能共享契约和测试，不能逐行复制实现。

## 1. 文件地图

| 文件 | 职责 |
| --- | --- |
| `pagetable.rs` | LA PTE、三级 walk、refill 路径、VMA、COW fork、销毁 |
| `asid.rs` | 固定 10-bit 用户 ASID、PGDL+ASID token 编码 |
| `kernel_global.rs` | 帧池、RAM/低 MMIO/PCI MMIO 恒等页表与 PGDL 激活 |
| `user_aspace.rs` | raw handle、地址空间互斥、CPU 历史集合、shootdown |
| `user_heap_mmap.rs` | `HeapBrk/MmapOps`、fault、madvise |
| `user_access.rs` | 跨页 copy、atomic u32、futex identity、probe |
| `kernel_elf.rs` | ELF64-LoongArch、lazy 段、解释器、musl 兼容补丁、栈 |
| `kernel_executable.rs` | ELF/shebang 递归解析 |
| `lib.rs` | 对外聚合、fork/COW/drop hook、自检 |

## 2. 地址空间与页表几何

```text
页大小               4 KiB
每级条目             512（9 bit）
层级                 3：VPN[2] -> VPN[1] -> VPN[0]
当前只建             level-0 4 KiB 叶
WaterOS 用户上界      0x0000_0080_0000_0000（512 GiB，不含）
用户 stack top       0x0000_007f_ffff_a000
token bits 0..47     PGDL 物理地址
token bits 48..57    10-bit ASID
```

`encode_token(pgdl,asid)` 会截取 PGDL 低 48 位和 ASID 低 10 位。根页表物理地址必须
4 KiB 对齐并低于 2^48；当前没有运行时检查被截掉的高位。

软件 walker 只取三个 9-bit VPN 索引。mmap 范围检查 `USER_VA_LIMIT`，但 user-copy 的
任意用户指针未统一验证地址架构合法性；新增 syscall 应先检查 `addr+len` 溢出及用户
下半区范围，不能让任意 64-bit 值直接进入软件 walker。

## 3. LoongArch PTE 位语义

| 位 | 名称 | 当前用途 |
| ---: | --- | --- |
| 0 | V | 有效叶 |
| 1 | D | Dirty；硬件实际写许可，D=0 的 store 触发 PME |
| 2..3 | PLV | 3 表示用户，0 表示内核 |
| 4..5 | MAT | 当前叶设 1：Coherent Cached |
| 7 | P | Present；`V && P` 判定叶 |
| 8 | W | WaterOS 软件“应可写”位，不是硬件最终写许可 |
| 9 | COW | 软件 COW 标志 |
| 10 | COW_WAS_WRITABLE | fork 前可写 |
| 61 | NR | Not Readable |
| 62 | NX | Not Executable |
| 63 | RPLV | 当前保留未来使用 |

`from_perm` 总是设置 `V|P|MAT_CACHED`：

- U -> PLV=3，否则 PLV=0；
- W -> 同时设置软件 W 和硬件 D；
- 无 R -> NR；无 X -> NX。

COW 不能照抄 RISC-V 的“清 W”：`prepare_cow` 必须同时清 W 和 D，以 D=0 让用户 store
产生 Page Modified Exception；解决后恢复 W+D。trap 层必须把 PME 正确分类到 COW，
普通 load/store page invalid 与 privilege fault 是不同异常。

### 3.1 目录项与叶项绝不能混用

LoongArch 目录项是**纯下一级表物理地址**：

```text
directory_pte = child_ppn << 12       // 不设 V/P/MAT
leaf_pte      = data_ppn << 12 | flags
```

硬件 `LDDIR` 会把目录低位当地址的一部分，给目录项加 `V` 会破坏 refill。空项必须全 0；
`walk_find` 用 `pte.0==0` 判空，用 `V&&P` 判叶。修改共用 PTE helper 时必须分别保留
`set_table` 和 `set`。

`LoongArch64Pte::ppn()`只保留 48-bit PA 对应的 36-bit PPN；超出范围会静默截断。

### 3.2 PROT_NONE

`PROT_NONE|U` 仍是 `V|P|PLV3|MAT|NR|NX` 叶，W/D 均为 0。软件认为地址已映射，硬件
读/写/执行均因 NR、D、NX受限。mprotect/unmap/destroy 因而仍能找到并处理该页。

### 3.3 unsafe 物理直访

`table_mut`、COW、用户 copy 和 ELF 都把 `ppn*4096` 当内核指针，依赖 LoongArch DMW/
恒等物理访问配置。若修改 DMW 或切换高半 direct map，必须统一替换全部物理直访。

与 Sv39 一样，`walk_find(&self)` 返回 `&'static mut PTE`，安全性依赖地址空间 cell 锁；
绕开 `with_user_aspace_mut*` 会产生别名可变引用和并发页表写。

## 4. `LoongArch64AddressSpace`

```text
root, asid
user_brk_start/current/max
mmap_anon_cursor, mmap_file_cursor, mmap_base
user_stack_bottom/top
lazy_file_vmas
shared_anon_vmas
shared_file_vmas
device_vmas
```

四类 VMA 的所有权语义与 common 手册一致。共享文件映射同时登记 shared-anon（共享
PPN/fork 分类）和 shared-file（writeback）；设备 VMA 的 lease 决定外部对象生命周期。

`unsafe impl Send/Sync` 只因为 `MultiprocessorSafeCell` 串行 loader 和页表。不可把结构
本身无锁共享给其他 CPU。

## 5. walk 与硬件 refill 路径

### 5.1 `walk_create/walk_find`

```text
walk_create VPN
  root(level2) empty -> 分配零页子表，set_table(纯 PA)
  nonempty leaf -> AlreadyMapped
  level1 同样处理
  level0 -> 返回叶槽
```

中间分配失败不会回滚已经挂入的目录；unmap 叶后也不剪枝空目录，全部到 destroy 才
释放。稀疏映射反复创建/删除会保留页表帧。

### 5.2 `ensure_lazy_refill_paths`

这是 LoongArch 特有的关键步骤。Linux 可让空目录指向共享 invalid lower-level table；
WaterOS 使用零目录。硬件 refill walker 若在高层遇到 0，无法走到未来将安装的叶槽，
因此登记 lazy VMA 时先为范围内每个 2 MiB leaf-table span 调一次 `walk_create`：

```text
register_lazy_file_vma
  -> validate/overlap
  -> ensure_lazy_refill_paths(start,end)
       -> 对每个 2MiB 边界创建 level2/level1 路径
       -> level0 PTE 保持 0
  -> 插入 LazyVmaSet
```

影响：

- “lazy” 只延迟数据帧，不延迟所有页表帧；4 GiB 稀疏 VMA约需 2048 个 leaf table，
  仅这些就约 8 MiB 物理内存；
- 中途 OOM 会留下已创建目录，但 VMA尚未登记；
- munmap lazy VMA 不回收目录，必须等地址空间 destroy；
- 新增任何不建叶 PTE 的 VMA，也要判断硬件 refill 是否需要提前铺目录。

这也是 LoongArch 与 Sv39 内存基线不同的主要原因之一。

### 5.3 `AddressSpaceOps`

- `map_page_to_ppn`：walk 后若叶槽 flags valid 则 `AlreadyMapped`；失败不接管输入 PPN。
- `unmap_page_to_ppn`：清叶并返回 PPN，不释放。
- `protect_page`：用 `from_perm` 重写 flags，会清 COW 位并重建 W/D/NR/NX。
- `translate_addr`：只接受 level 0 叶，高层叶返回 `Unsupported`。
- `satp_value` 字段名是跨架构历史名称，实际返回 PGDL+ASID token。
- trait `fork()` 不支持；正式路径走可变 `fork_cow()`。

低层方法不自动失效 TLB。

## 6. 内核 PGDL 启动链

```text
kernel_mm::init(_dtb_pa, ram_end)
  -> linker kernel_end 到 ram_end 初始化 frame allocator
  -> new_kernel：ASID 0
  -> 4KiB 恒等映射 RAM [0x90000000, ram_end) 为 RWX
  -> 映射 low MMIO [0x10000000,0x30000000) 为 RW
  -> 映射 PCI MMIO [0x40000000,0x80000000) 为 RW
  -> 从 allocator 取一页作为恒等 probe
  -> 安装 PGDL，enable_paging，验证软件翻译和实际读写
  -> BootOnceCell 保存内核地址空间，发布 token
  -> 注册 drop/CPU/mapping-snapshot hooks
```

当前限制：

- `_dtb_pa` 完全未使用，DTB blob 未像 RISC-V 那样从 frame allocator 排除；若 DTB 位于
  `[kernel_end,ram_end)` 且固件不再复制，后续分配可能覆盖它。应按 FDT total_size 保留。
- RAM 全部 RWX，没有内核 W^X。
- 低 MMIO+PCI 窗口共 1.5 GiB，加 RAM 都用 4 KiB PTE，内核页表本身占若干 MiB物理帧。
- probe 页由 allocator 正式分配但从未释放，等价于永久保留一帧；最好在内核 cell 中
  明确记录所有权，或 probe 后释放且移除额外引用。
- `map_anon_range_user` 不清零新帧，map 异常时也可能泄漏；三个 legacy kernel mapping
  helper 均不自动 TLB flush。
- 内核 PGDL 永不 Drop，这是预期；不能运行期重复 init。

用户地址空间**不映射整段内核 RAM**。`from_elf_path/from_elf_bytes` 中明确禁止复制该
映射：旧做法使 destroy 无法从无 U 子树正确辨认共享结构，每次 exec 可泄漏约 2 MiB
页表帧。trap 进内核依赖架构 DMW/PGDL 切换链路，详见架构手册。

## 7. ASID 和 TLB

### 7.1 ASID

当前硬编码 `ASID_BITS=10`，ASID 0 为内核，用户可用 1..1023。位图线性扫描，耗尽返回
`OutOfMemory`。没有像 RISC-V 一样运行时探测 ASID width；若硬件实现位数不同，token
截断/复用会错误。bring-up 时应读取 ASIDBITS CSR/架构字段并验证至少 10 位，或动态
限制 allocator。

只有确认所有历史 CPU TLB 已失效才能 release ASID。shootdown 失败时 destroy 永久
退休编号；重复失败最多会耗尽 1023 个编号。

### 7.2 raw handle 与 tombstone

`into_handle` 把 `Box<UserAddressSpaceCell>` 变为 `usize`：cell 包含地址空间锁、dropped
和只增不减的 `tlb_cpus`。LoongArch cell 不另存 token；destroy 使用保守 All flush。

`destroy` 不 `Box::from_raw`，只销毁页表/VMA并把 cell 永久留作 stale-handle tombstone。
因此每次 fork/exec/exit 都在线性消耗内核 heap。`forkheavy` 出现“用户页都释放但 heap
used 单调上涨”时应首先统计 tombstone；正确方案是 generation+refcount/epoch 句柄表，
安全等待引用退出后回收 slot。

`cell(handle)` 只拒绝 0，任意其他坏指针会被 unsafe 解引用；handle 必须永远是内核可信
对象，不能进入用户 ABI。

### 7.3 shootdown

协议与 Sv39 的 pending/completed 全局串行方案相同，但 local flush 目前保守使用 All：

```text
tlb_cpus & online - self
  -> platform remote flush
  -> Unsupported/失败：sequence + per-CPU pending + IPI
  -> 远端 All flush并 completed
  -> 发起者最多 spin 10,000,000 次
```

等锁时主动处理本 CPU pending，避免关中断死锁。`mark_active` 首次在某 CPU运行该地址
空间前执行 local All，清除 ASID 复用遗留；`mark_inactive` 不清历史位。

当前普通修改路径忽略 shootdown false，超时后仍可能返回 syscall 成功，远端 stale TLB
未解决；destroy 才通过退休 ASID保安全。`with_user_aspace_mut_and_page_flush` 在闭包 Err
时不会执行 local flush。sequence wrap 也没有模序比较。

## 8. fork、COW 与销毁

### 8.1 fork 分类

```text
fork_user_aspace
  -> with_user_aspace_mut_and_flush(parent)
  -> duplicate lazy/shared-file loaders
  -> alloc child ASID/root
  -> fork_table
       kernel PLV0 leaf：不增 ref，直接共享
       device：不增通用 ref，flags 原样
       shared-anon：增 ref，flags 原样
       private read-only/cache：增 ref，flags 原样
       private writable：增 ref，父子清 W+D，置 COW 两位
  -> clone VMA metadata/lease -> child handle
```

共享文件正常创建时也登记 `shared_anon_vmas`，所以 fork 避开 COW；重构双表关系时必须
显式把 shared-file 纳入分类。fork 中途失败销毁子树，但父已经 COW 的页不回滚；refcount
回到 1 后父首次写可原地恢复。

### 8.2 COW fault

LoongArch store 对 D=0 叶触发 PME：

```text
trap PME -> kernel_mm_impl::handle_cow_fault
  -> page-flush wrapper
  -> handle_cow_page
       验证 level0 + leaf + COW + WAS_WRITABLE
       ref<=1：原 PPN 恢复 W+D
       ref>1：alloc/copy/decrement old/PTE切新 PPN
  -> local page invalidate；changed 才 remote
```

`handle_cow_fault_no_flush` 还识别“别的 CPU已经解决、当前 CPU持 stale D=0 TLB”的情况：
当前 PTE 若是 user+writable+dirty leaf，仍返回 handled，让包装层 flush。

新页分配后若旧 ref decrement 失败，新页未释放且 PTE未提交；`ensure_private_for_write`
同样如此。用户 copy/CAS 直接调用会在地址空间锁内换 PPN并仅做 local 全量 flush，没有
远端 shootdown，多线程可继续看到旧 PPN；应返回 changed 并统一走 page wrapper。

### 8.3 destroy

`destroy_and_take_asid` 先写回所有 shared-file；失败只 warn，然后仍销毁页表、释放用户
非设备叶引用、释放目录、drop loaders/leases。dirty 数据可能因此丢失。随后 local+remote
All flush；成功归还 ASID，失败永久退休。cell tombstone不回收。

`destroy_table` 以 PLV3 判断用户叶，以 device VMA排除外部 PPN。`shared_anon_vmas` 参数
当前没有影响释放：共享页每个地址空间各释放自己的一次 ref，这是正确引用语义。

## 9. brk、mmap、fault 与 madvise

公共行为和风险详见 Sv39/common 手册；本实现的 API结构基本对应：

| 类型 | 建立方式 | 元数据 |
| --- | --- | --- |
| private anonymous | lazy 数据页 + eager refill目录 | `LazyFileVma::Anonymous` |
| shared anonymous | eager 零页 | `SharedAnonVma` |
| private file | lazy/eager | `LazyFileVma` 或普通 owned 页 |
| shared file | eager | `SharedAnonVma + SharedFileVma` |
| device | eager 外部 PPN | `DeviceVma + lease`，SHARED且不可 X |

`brk` 墽长当前 eager 分配零页；stack/brk fault 可补被 madvise 丢弃的洞。fault 顺序是
stack -> brk -> common lazy。COW由 PME trap 单独处理。

`MAP_FIXED` 先破坏旧目标，失败无完整 rollback；`FIXED_NOREPLACE` 主要只在匿名路径。
非固定的非空 hint 多数直接拒绝。选址最多扫描 4 GiB，避开 PTE、stack、lazy和
shared-anon；LoongArch 用户页表没有 Sv39 kernel trampoline window。

`munmap` 先 shared-file writeback，再删 PTE/VMA；后半 loader duplicate 失败会留下
PTE/VMA不一致。`mprotect` 先改 lazy metadata，再逐叶，错误可能部分提交。`mremap` 的
分配复制和 FIXED 事务缺口见 common 手册。

`MADV_DONTNEED/FREE` helper 用普通 allocator 回收叶；调用前必须用
`madvise_range_shared_or_file` 排除 shared/device，否则会错误释放外部 PPN。prefault
完整 2 MiB stack 或大 lazy VMA会同时增加数据帧压力。

## 10. ELF、解释器与 LoongArch 兼容补丁

### 10.1 布局

```text
EM_LOONGARCH             258
ET_DYN main bias         0x0040_0000（path loader）
interpreter base         0x7000_0000
preferred mmap base      max(image_end+64MiB, 0x10000000)
stack top                0x0000007ffffffa000
stack reserve            2 MiB
stack premap             16 页 + stack_top 兼容页
signal trampoline        下一页：a7=139; syscall 0
```

源码中 `map_user_stack` 的旧注释仍写“256KiB”，实际常量是 2 MiB，以常量为准。地址空间
记录的 stack top 包含额外兼容页，signal trampoline 不属于 stack。

path loader 只读 ELF header+phdrs，支持 ET_EXEC/ET_DYN、PT_INTERP、lazy/eager 段和
只读缓存。内存 `from_elf_bytes` 是兼容路径：它没有读取 `e_type`/应用 PIE load bias，也
不装动态解释器，不能替代正式 path exec 测试。

### 10.2 musl scheduler shim

若解释器路径恰好为 `/musl/lib/libc.so`，loader 在四个固定 offset 检查
`li.w a0,-ENOSYS` marker，然后用物理直写把 stub 改成对应 scheduler syscall：

```text
li.w a7, nr
syscall 0
slli.w a0, a0, 0
jirl zero, ra, 0
```

这是与特定 musl 二进制布局绑定的兼容补丁：升级 libc 后 offset 会变化；marker 不符时
只 warn 并跳过。新增 syscall 后不应继续靠 patch libc，优先实现正确 syscall number/
ABI。若必须修改指令，还要确保在用户执行前完成并考虑 LoongArch I-cache 同步要求。

### 10.3 用户页表边界

正式 loader 不调用已废弃的 `map_kernel_ram_identity`。把内核 RAM 映射进每个用户 PGDL
曾导致每次 exec 大量页表帧泄漏；不要为“内核访问用户页方便”恢复它。内核访问物理页
应依赖 DMW/direct-map，trap 通过架构切换协议完成。

## 11. 用户 copy、atomic 与 futex

copy 每页软件翻译，必要时触发 demand fault，检查 PLV3对应的 U 和 R/W，再物理直拷。
copy-to 返回部分完成数；copy-from 错误时 destination 可能已有前缀但返回类型不表达进度。

u32 atomic 要求 4-byte 对齐且不跨页，使用 SeqCst。CAS 会处理 lazy和 COW，返回观察到
的旧值。

shared futex key 是完整物理地址。`shared_vma_contains` 只查 shared-anon；共享文件当前
通过双登记也能命中。重构时要显式加入 shared-file，否则跨进程文件 futex会被当 private。

已知 SMP 缺口与 Sv39 相同：user copy/CAS 触发 `ensure_private_for_write/handle_cow_fault`
换 PPN后没有走远端 shootdown包装。两架构应以同一 API修复，但各自 local invalidate
指令不同。

## 12. 新增 syscall 实例：页驻留查询

以实现 `mincore` 为例：

1. ABI 层验证页对齐、`addr+len`、`USER_VA_LIMIT` 和输出 vector；
2. 用 `with_user_aspace_mut` 只读遍历，无需 TLB flush；
3. `translate_addr` 决定 resident，VMA/brk/stack 决定地址是否合法；
4. 特别注意 lazy VMA已铺目录但叶 PTE=0，目录存在不等于 resident；
5. 退出地址空间锁后再 `copy_to_user_progress` 写结果，避免嵌套同一锁；
6. 统一 `MmError -> errno`。

修改 PTE 的新 syscall 应返回 changed，并由
`with_user_aspace_mut_and_flush_if_changed` 完成 local+remote。外部页必须同步定义 fork、
munmap、mprotect、madvise、destroy 和 lease/refcount。

## 13. 故障定位

### lazy fault 反复或 refill 异常

确认 `register_lazy_file_vma` 是否执行 `ensure_lazy_refill_paths`，目标 2 MiB span 的两级
目录是否为纯 PA，叶是否 0，fault 后叶是否 V|P，local invtlb 是否执行。目录项误设 V
是 LoongArch 专属高概率错误。

### 写页总进 PME / COW 不生效

检查 PagePerm W、软件 W、硬件 D、COW/WAS_WRITABLE四者。普通 writable 必须 W+D；
COW 必须清 W+D；真正只读没有 COW。检查 trap 是否把 PME路由 COW而非直接 SIGSEGV。

### forkheavy heap 或帧持续上升

分别统计永久 cell tombstone、lazy refill目录、全局 readonly cache、页表中间页和 VMA
loader。LoongArch 大 lazy VMA即使无 RSS也消费目录帧，不能套用 Sv39基线。

### ASID 很快 OOM

统计 shootdown failure/retired ASID 和活地址空间。硬编码仅 1023 用户编号；一次次超时
会永久退休。不要把 ASID耗尽误报成物理页 OOM。

### 地址空间销毁写回失败

沿 `destroy -> sync_shared_file_vmas -> loader.write_page/flush -> VFS` 记录文件 identity、
offset 和 errno。确认 mount generation 未先推进、没有地址空间/VFS锁反转。

### 用户程序莫名执行旧指令

除 TLB 外检查 musl shim 或其他 ELF patch 后的 I-cache同步、固定 offset/marker、解释器
路径是否命中。不要把硬编码补丁失败当通用 syscall错误。

## 14. 自回归矩阵

- PTE：目录纯 PA、叶 V/P、NR/NX、W/D、PROT_NONE、PA 48-bit 边界；
- refill：单页、跨 2 MiB、4 GiB lazy、目录 OOM、munmap后目录驻留、destroy回收；
- COW：ref1/ref>1、PME、并发 stale D=0、copy/CAS触发、shared/device不 COW；
- ASID/TLB：1..1023、耗尽/复用、remote/fallback/timeout/offline、首次 CPU All flush；
- mmap：private/shared/file/device、FIXED/NOREPLACE、writeback失败、mprotect/mremap失败注入；
- ELF：ET_EXEC/PIE path、PT_INTERP、lazy/eager、BSS、2 MiB stack、signal trampoline、
  musl marker匹配/不匹配、from-bytes限制；
- user copy：跨页部分完成、权限、溢出、高地址、COW远端可见性、shared futex；
- 启动：DTB保留、RAM/low-MMIO/PCI窗口、probe所有权、PGDL/ASID CSR一致；
- 压力：SMP=8 fork/exec/exit，分别记录 heap tombstone、free frames、refill table pages、
  cache resident、ASID retired。

现有自检只覆盖基础 map/protect/unmap、一个 lazy fault、cache refcount 和单地址空间 COW
copy，不覆盖硬件 refill异常、真实 PME/SMP shootdown、musl patch、DTB或失败事务。

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=la PROFILE=pre
make check ARCH=rv PROFILE=pre
git diff --check
```

## 15. 修改检查清单

- [ ] 目录项保持纯 PA，叶项才使用 V/P/MAT/权限。
- [ ] lazy 元数据创建前铺 refill 路径，并对失败/删除设计目录回收。
- [ ] 写权限同时正确维护 W/D，COW由 PME进入。
- [ ] 用户地址和 PGDL/PPN不发生位截断或非用户区别名。
- [ ] PTE换页经 local+remote shootdown，超时不静默成功。
- [ ] handle cell 能在安全代际协议下回收，不永久 tombstone。
- [ ] DTB、probe、device、shared/cache帧所有权明确。
- [ ] 新 VMA 接入选址、refill、fault、fork、munmap、mprotect、madvise、destroy。
- [ ] 不把内核 RAM 页表复制进用户地址空间。
- [ ] LA/RV API语义共同回归，架构细节分别验证。
