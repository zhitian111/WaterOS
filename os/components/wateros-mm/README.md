# wateros-mm

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-mm` 是 WaterOS 的内存管理聚合层。它对外 re-export [`api`]（语义契约）、
`frame_alloctor`（物理帧分配）与经 `kernel_mm` 暴露的 bring-up 能力；具体 Sv39 / LoongArch64 /
桩代码**不**以 `mm_impl` 别名整包导出，避免页表实现细节泄漏到依赖方。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | re-export `api`、`frame_alloctor`、`kernel_mm`、`user_access`、`mempolicy`；经 `active_mm_impl` 互斥选择 Sv39 / LoongArch64。 |
| MM API | `mm-api/api-v0/` | 虚拟/物理地址、页权限、地址空间与 mmap/brk/用户访问等 trait 契约，不实现具体页表。 |
| 帧分配器 | `mm-frame-alloctor/` | 栈式物理帧分配器与 `OwnedPhysPage`，提供 `PhysPageNum` 粒度帧。 |
| 共享实现 | `mm-impl/common/` | 各 arch 实现共享的 ELF 装载、mmap/mremap、按需零页辅助。 |
| Sv39 实现 | `mm-impl/impl-sv39/` | RISC-V Sv39 页表、用户地址空间与设备 mmap。 |
| LoongArch64 实现 | `mm-impl/impl-loongarch64/` | LoongArch64 页表与用户地址空间。 |

## 实现说明

- 语义层页大小固定为 **4 KiB**（`api::addr::PAGE_SIZE`），与 RISC-V Sv39 常用叶子页一致；大页
  不在本阶段 API 中表达。
- `kernel_mm` 下各 impl 依赖**恒等映射或等价的物理线性访问**，以便页表 walk 时把 PPN 当可写
  指针用；更换映射模型时需同步改 `mm-impl`。
- `impl-sv39` 与 `impl-loongarch64` 互斥：Cargo features 不支持在依赖链上去重，本文件通过
  `cfg` 确保仅一个 impl 的符号被编译进当前 crate。
- API 层只定义契约（`AddressSpaceOps`、`MmapOps`、`HeapBrk`、`UserMemoryOps` 等），不实现页
  table；Sv39 的 PTE 编码与 walk 是 crate 内部实现，不对外暴露。
- 用户地址空间句柄：`LoadedElf::user_aspace_ptr` → 可调用 `HeapBrk` / `MmapOps` 的实例
  （`active_mm_impl::user_aspace`）。
- 设备 mmap：`MmapKind::Device` + `DeviceMapping` + lease；解除映射只删 PTE，不把设备页交给
  普通 frame allocator；fork 共享设备页，不执行 COW。
- 惰性缺页：匿名 mmap 用 `ZeroAnonLoader`（按需零页），PT_LOAD 段用 `fill_elf_load_page`，
  避免饥渴分配整段物理帧。
- 多核：页表变化复用 active CPU mask 与 TLB shootdown（`handle_tlb_shootdown_ipi`）；COW 由
  `handle_cow_fault` 处理。

## 调用链路

初始化与 ELF 装载：

```text
kernel_mm::init
  -> 初始化全局页表与帧分配器（kernel_satp / kernel_global）
  -> from_elf_bytes / from_elf_path -> LoadedElf
  -> prepare_elf_user_stack -> 写入 argc/argv/envp/auxv -> 返回初始 sp
```

用户地址空间操作：

```text
sys_mmap / sys_brk / sys_mprotect / sys_mremap
  -> user_aspace -> HeapBrk / MmapOps
  -> 惰性 loader（ZeroAnonLoader / fill_elf_load_page）
  -> 缺页时 handle_lazy_page_fault / handle_cow_fault

设备 mmap
  -> MmapOps::mmap_device -> DeviceMapping + lease
  -> munmap / 进程退出只删 PTE，不回收设备页
```

fork / TLB：

```text
fork_user_aspace        // 复制地址空间，设备页不 COW
handle_tlb_shootdown_ipi // IPI 后各 CPU 刷新本地 TLB
```

## 各实现功能

### mm-api / 语义契约

`mm-api/api-v0/src/` 下的文件：

- `addr.rs`：`PhysPageNum` / `VirtAddr` / `VirtPageNum` / `PAGE_SIZE` 与地址分解。
- `error.rs` / `flags.rs` / `perm.rs`：`MmError`、`MmapFlags`、`PagePerm`。
- `address_space.rs`：`AddressSpaceOps` 契约。
- `frame_allocator.rs`：`PhysicalFrameAllocator` 契约。
- `user_access.rs`：用户缓冲区访问契约（syscall 路径）。
- `brk.rs`：`HeapBrk` 堆增长契约。
- `elf_user_stack.rs`：用户栈上写 argc/argv/envp/auxv。
- `executable.rs` / `kernel_bringup.rs`：`LoadedElf`、`LoadElfError`、`RootVolumeReadError` 等
  bring-up 契约。
- `kernel_satp.rs`：内核页表切换契约。
- `mempolicy.rs`：内存策略。
- `mmap.rs`：`MmapOps`、`MmapKind::Device`、`DeviceMapping`、`DemandPageLoader`。
- `user_aspace_lifecycle.rs`：用户地址空间生命周期。

### mm-frame-alloctor / 物理帧分配器

`mm-frame-alloctor/src/lib.rs`：

- 栈式分配（`impl-stack` feature）：从帧池顺序分配**物理页号递减**的连续帧。
- `OwnedPhysPage`：独占一页（分配清零、`frame_id`、`as_bytes`/`as_bytes_mut`，`Drop` 恰好回收
  一次）；使用方不能取得可复制的所有权句柄。

### mm-impl/common / 共享实现

`mm-impl/common/src/lib.rs`：

- `ZeroAnonLoader`：匿名 mmap 的按需零页 loader。
- `fill_elf_load_page`：PT_LOAD 惰性缺页，按页从 ELF 文件区间填充。
- 共享的 mmap/mremap、`load_or_get_readonly_mmap_page` 等辅助。
- 本 crate 不对外暴露稳定契约，语义边界以 `mm-api` 为准。

### mm-impl/impl-sv39 / RISC-V Sv39 实现

`mm-impl/impl-sv39/src/` 下的文件：

- `lib.rs`：模块聚合与自检（依赖已初始化帧分配器）。
- `pagetable.rs`：`Sv39AddressSpace`、PTE 编码与页表 walk。
- `asid.rs`：ASID 管理。
- `kernel_elf.rs` / `kernel_executable.rs`：内核 ELF / 可执行文件装载。
- `kernel_global.rs`：内核全局页表（恒等映射与 RAM 上界）。
- `user_access.rs`：`Sv39UserMemoryOps` 与用户虚拟地址探测。
- `user_aspace.rs`：用户地址空间实例。
- `user_heap_mmap.rs`：`mmap_device`（设备 DMA 页映射 + lease）、munmap/mprotect/mremap。

### mm-impl/impl-loongarch64 / LoongArch64 实现

- 文件结构与 Sv39 一致（`pagetable.rs` / `asid.rs` / `kernel_*` / `user_access.rs` /
  `user_aspace.rs` / `user_heap_mmap.rs`），页表格式为 LoongArch64。
