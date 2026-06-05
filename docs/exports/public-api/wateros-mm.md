# wateros-mm 公共 API 快照

## 用途

列出 **`wateros-mm`** 聚合层对 **`api`**（契约）、**`frame_alloctor`**（物理帧）、**`mm_impl`**（Sv39/dummy）与 **`kernel_mm`**（内核全局页表与用户装载）的再导出，并标明根 crate **`wateros`** 通过 feature 叠加后的 **主线差异**（如 **`from_elf_bytes`** 是否可见）。

## 事实来源

- [`os/components/wateros-mm/Cargo.toml`](../../os/components/wateros-mm/Cargo.toml)
- [`os/components/wateros-mm/src/lib.rs`](../../os/components/wateros-mm/src/lib.rs)
- [`os/Cargo.toml`](../../os/Cargo.toml)（`impl-sv39`、`qemu-riscv64-opensbi` 向子 crate 传递）
- [`os/components/wateros-mm/mm-api/api-v0/src/lib.rs`](../../os/components/wateros-mm/mm-api/api-v0/src/lib.rs)

## 组件默认 feature

| 项 | 说明 |
|----|------|
| **`wateros-mm` `default`** | `api-v0` + **`impl-sv39`**；**`frame_alloctor`** 子依赖默认还带 **`impl-stack`**（栈式帧分配器）。 |
| **`qemu-riscv64-opensbi`** | 空 feature 名，用于与 **`impl-sv39`** 组合 **`kernel_mm`** 中 **Sv39 真实 bring-up** 分支（见下）。 |

根 **`wateros`** 默认 feature 含 **`impl-sv39`** 与 **`qemu-riscv64-opensbi`**，故主线构建下 **`kernel_mm`** 走 **Sv39 + QEMU OpenSBI** 分支。

## 聚合层导出

| 项 | 说明 |
|----|------|
| **`pub use api_v0 as api`** | 整包 **`wateros-mm-api-v0`**：`addr`、`error`、`perm`、`flags`、`frame_allocator`、`address_space`、`user_access`、`brk`、`mmap`、`kernel_bringup` 等子模块及根级再导出（含 **`PhysicalFrameAllocator`**、**`test`**）。 |
| **`pub use frame_alloctor`** | 帧分配错误/结果类型、默认 **`impl-stack`** 下的 **`StackFrameAllocator`**、**`init_frame_allocator`**、**`frame_alloc`** / **`frame_dealloc`**、**`frame_mem_stats()`**（`FrameMemStats`）等。 |
| **`mm_impl`** | **`#[cfg(feature = "impl-sv39")]`** → **`impl_sv39`**；**`#[cfg(feature = "impl-dummy")]`** → **`impl_dummy`**（二者勿同时用于别名）。 |
| **`kernel_mm`** | 再导出 **`DEFAULT_USER_ELF_PATH`**、**`LoadElfError`**、**`LoadedElf`**。 |
| **`kernel_mm`（Sv39 + `qemu-riscv64-opensbi`）** | **`impl_sv39::kernel_mm_impl`**：**`init`**、**`kernel_satp`**、**`from_elf_path`**、**`from_elf_bytes`**、**`ensure_user_execute_for_kernel_va`**、**`map_anon_range_user`**、**`map_identity_range_user`**。 |
| **`kernel_mm`（非上述组合）** | **`impl_dummy::kernel_mm_impl`**：同上集合但 **无 `from_elf_bytes`**。 |
| **`user_aspace`（`impl-sv39`）** | **`with_user_aspace_mut(handle, f)`**：将 **`LoadedElf::user_aspace_ptr`** 解析为可调用 **`HeapBrk`/`MmapOps`** 的地址空间；**不**封装 Linux syscall 语义（由 **`wateros-syscall`** 拼合）。 |
| **`test_with_range`** | 根级自检：传入 **`BasePPN`** 闭开区间，串联 **`api::test`**、**`frame_alloctor::test_with_range`** 与（若启用 Sv39）**`impl_sv39::test_with_range`**。 |

## 缺口说明

- **`kernel_mm`** 行为依赖根 feature 组合；单独编译 **`wateros-mm`** 而不开 **`qemu-riscv64-opensbi`** 时与 QEMU 主线文档叙述不一致，需在集成侧对齐。

## 维护要求

聚合导出、**`kernel_mm`** 条件分支或默认 feature 链变化时，同步更新本文件、**`docs/exports/features/wateros-mm.md`** 与 **`docs/architecture/snapshot.md`** 中 MM 相关句段。
