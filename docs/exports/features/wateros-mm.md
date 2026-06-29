# wateros-mm 已实现功能

## 用途

记录 `wateros-mm` 一级组件在当前快照下已具备的能力、按 feature 选择的实现差异，以及仍为桩或语义子集的部分。

事实来源：`os/components/wateros-mm/Cargo.toml`、`src/lib.rs`、各子 crate 源码；根 `wateros` 通过 `mm/impl-sv39` 或 `mm/impl-loongarch64` 选择平台实现。

## 聚合层（`wateros-mm`）

| 能力 | 状态 | 说明 |
|------|------|------|
| API 契约 re-export | 已实现 | `mm::api` → `wateros-mm-api-v0` |
| 物理帧分配 | 已实现 | `mm::frame_alloctor`，默认 `impl-stack` |
| 内核 bring-up | 已实现 | `mm::kernel_mm::{init, kernel_satp, ...}` |
| 用户 ELF 装载 | 已实现 | `from_elf_path` / `from_elf_bytes` / `load_program_from_path` |
| 用户栈 argc/argv/auxv | 已实现 | `prepare_elf_user_stack` |
| fork COW 地址空间 | 已实现 | `fork_user_aspace`、`handle_cow_fault` |
| 用户缺页处理 | 已实现 | `handle_user_page_fault`（栈/brk/lazy mmap） |
| madvise 丢弃页 | 已实现 | `madvise_discard_pages` |
| 用户缓冲区访问 | 已实现 | `user_access` / `ActiveUserMemoryOps` |
| NUMA 策略桩 | 已实现 | `mempolicy` 单节点逻辑 |
| 自测入口 | 已实现 | `test_with_range` |

默认 feature `api-v0` 仅链接 `impl-dummy` 页表桩；真实 MM 须启用 `impl-sv39` 或 `impl-loongarch64`。

## API 契约层（`wateros-mm-api-v0`）

| 模块 | 状态 | 说明 |
|------|------|------|
| `addr` | 已实现 | 4 KiB 页、Virt/Phys 地址与页号 |
| `perm` / `flags` | 已实现 | 页权限与 mmap 语义标志 |
| `address_space` | 已实现 | `AddressSpaceOps` trait |
| `mmap` / `brk` | 已实现 | `MmapOps`、`HeapBrk` trait |
| `user_access` | 已实现 | `UserMemoryOps` trait |
| `kernel_bringup` | 已实现 | `LoadedElf`、装载错误类型 |
| `executable` | 已实现 | shebang 解析、busybox 路径辅助 |
| `elf_user_stack` | 已实现 | RISC-V 用户栈布局 |
| `kernel_satp` | 已实现 | 内核地址空间 token 缓存 |
| `user_aspace_lifecycle` | 已实现 | 任务 exit 释放钩子注册 |
| `mempolicy` | 桩 | 单节点 `MPOL_DEFAULT` |

## 页表实现（`impl-sv39` / `impl-loongarch64`）

| 能力 | Sv39 | LoongArch64 |
|------|------|-------------|
| 三级 4 KiB 页表 | 是 | 是 |
| 内核恒等 RAM/MMIO 映射 | 是 | 是 |
| 用户 ELF PT_LOAD 映射 | 是 | 是 |
| 惰性文件/匿名 mmap | 是 | 是 |
| 共享匿名 mmap | 是 | 是 |
| brk / munmap / mprotect | 是 | 是 |
| mremap 子集 | 是 | 是 |
| fork COW | 是 | 是 |
| 软件 walk 用户拷贝 | 是 | 是 |
| `satp` / PGDL 编码 | Sv39 MODE=8 | `root * PAGE_SIZE` |

## 物理帧分配（`wateros-mm-frame-alloctor`）

| 能力 | 状态 |
|------|------|
| 栈式 LIFO 分配器 | 已实现（`impl-stack`） |
| 引用计数（COW） | 已实现 |
| `/proc/meminfo` 统计 | 已实现（`frame_mem_stats`） |
| dummy | 桩（无真实分配） |

## 共享实现辅助（`wateros-mm-impl-common`）

ELF 三次读稳定、`mremap`、按需零页 loader、映射辅助函数等；不对外导出。

## 已知缺口与语义子集

- 页大小固定 **4 KiB**，无大页路径。
- `MapFlags::SHARED` 仅部分场景（共享匿名）完整；其它共享文件语义可能受限。
- `impl-dummy` 不装载 ELF、不 fork。
- NUMA 无真实拓扑，仅满足 bring-up syscall 返回值。
- 帧分配器为单核栈式实现，无 buddy/slab 分层。
- LoongArch 与 RISC-V 的 trap 保留区检测策略不同（符号 vs RAM 区间）。
