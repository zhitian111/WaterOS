# wateros-mm 功能快照

## 用途

记录 **`wateros-mm`** 在默认 **`impl-sv39`** 与帧分配器路径下的地址空间契约、内核 bring-up 辅助（ELF 装载、全局内核页表）及与根 feature（如 **`qemu-riscv64-opensbi`**）的组合行为。

## 事实来源

- `os/components/wateros-mm/Cargo.toml`
- `os/components/wateros-mm/src/lib.rs`
- `os/components/wateros-mm/mm-api/api-v0/`
- `os/components/wateros-mm/mm-impl/impl-sv39/`、`mm-impl/impl-dummy/`
- `os/components/wateros-mm/mm-frame-alloctor/`
- `os/feature-tree.txt`

## Feature 链

- **`default`**：`api-v0`、`impl-sv39`。
- **`api-v0`**：打开 **`impl-dummy`**、**`impl-sv39`** 子 crate 的 api 联动（见各子 **`Cargo.toml`**）。
- **`impl-sv39`**：RISC-V Sv39 地址空间实现。
- **`qemu-riscv64-opensbi`**：根 crate 转发；用于与 QEMU 物理 RAM 上界等 bring-up 常量对齐（与聚合层条件导出相关）。
- **`wateros-mm-frame-alloctor`**：**`default`** 含 **`impl-stack`**（栈式物理帧分配器）。

## api-v0 已暴露能力（摘要）

- **地址与错误**：**`PAGE_SIZE`**、**`VirtAddr` / `PhysAddr` / `VirtPageNum` / `PhysPageNum`**、**`MmError` / `MmResult`**。
- **地址空间契约**：**`AddressSpaceId`**、**`AddressSpaceOps`**（`satp_value`、`map_page_to_ppn`、`unmap_page_to_ppn`、`protect_page`、`translate_addr` 及带分配器的默认方法）。
- **帧分配契约**：**`PhysicalFrameAllocator`** 及帧分配相关类型。
- **用户内存与堆扩展契约**：**`UserMemoryOps`**、**`HeapBrk` / `BrkRegion`**、**`MmapOps` / `MmapRequest` / `MmapKind`**（实现厚度因 impl 而异）。
- **内核 bring-up**：**`kernel_bringup`** 模块（**`DEFAULT_USER_ELF_PATH`**、**`LoadElfError`**、**`LoadedElf`** 等），与 **`impl-sv39`** 中 ELF 解析衔接。

## impl-sv39 与 impl-dummy

- **`impl-sv39`**：**`Sv39AddressSpace`** 实现 **`AddressSpaceOps`**；**`kernel_elf`**（**`from_elf_path`**、**`from_elf_bytes`** → **`LoadedElf`**）、**`kernel_global`**（全局 **`Sv39AddressSpace`**、**`init`**、**`kernel_satp`**、恒等/用户映射辅助、**`phys_ram_end_exclusive`** 等）。**`Drop`** 当前仅回收根页表，注释说明未递归回收中间页表（早期简化）。
- **`impl-dummy`**：**`kernel_mm_impl`** 桩：**`init`** 空操作、**`kernel_satp`** 恒 0、映射无操作、**`from_elf_path`** 固定错误类；**无** **`from_elf_bytes`**。

## 帧分配器（impl-stack）

- **`StackFrameAllocator`** 实现 **`PhysicalFrameAllocator`**；提供 **`init_frame_allocator`**、**`frame_alloc`** / **`frame_dealloc`** 及带 **`MmResult`** 的变体等全局入口。

## 聚合层注意点

- **`kernel_mm`** 模块：仅在 **`impl-sv39`** 与 **`qemu-riscv64-opensbi`** **同时**启用时从 **`impl_sv39::kernel_mm_impl`** 带出 **`from_elf_bytes`**；否则走 dummy 聚合路径且不导出该符号（与 dummy 能力一致）。

## 明确未覆盖

- **`brk` / `mmap` / `UserMemoryOps`** 在 **`impl-sv39`** 上的完整用户态语义落地（需与 task、syscall 联调对照）。
- 页表递归回收与更完整的地址空间生命周期管理。

## 维护要求

默认 feature、**`kernel_bringup`** 或 **`impl-sv39`** 全局映射策略变化时，同步更新本文件、**`docs/architecture/snapshot.md`** 与 **`docs/guides/mm-validation.md`**（若涉及自检路径）。
