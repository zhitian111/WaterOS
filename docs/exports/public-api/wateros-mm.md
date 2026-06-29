# wateros-mm 公共 API

## 用途

描述根内核与其它一级组件通过 `wateros-mm` 聚合 crate **实际使用**的导出符号（非裸 `api-v0` 全量目录）。

事实来源：`os/components/wateros-mm/src/lib.rs`；根 `os/Cargo.toml` 中 `mm = { package = "wateros-mm", ... }`。

## 顶层 re-export

```text
mm::api              // wateros-mm-api-v0 别名
mm::frame_alloctor   // 物理帧分配聚合
mm::mempolicy        // NUMA 策略辅助（单节点）
mm::user_aspace      // 用户地址空间句柄解析（impl 条件导出）
mm::user_access      // 用户内存访问类型
mm::ActiveUserMemoryOps  // 当前 arch 的 UserMemoryOps 实现别名
mm::kernel_mm        // 内核与用户装载、页故障、fork 等
mm::test_with_range  // 组件自测
```

## `mm::api`（契约层，稳定语义）

主要被 syscall、task、trap 直接引用的子模块：

| 路径 | 典型消费者 |
|------|------------|
| `api::addr::{VirtAddr, PhysPageNum, PAGE_SIZE, ...}` | 全内核 |
| `api::perm::PagePerm` | syscall mmap/mprotect |
| `api::flags::MapFlags` | syscall mmap |
| `api::error::{MmError, MmResult}` | syscall 错误映射 |
| `api::mmap::{MmapOps, PageFaultAccess, MmapRequest, ...}` | syscall、trap |
| `api::brk::HeapBrk` | syscall brk |
| `api::kernel_bringup::{LoadedElf, LoadElfError, ...}` | bring-up、exec |
| `api::executable` | shebang / busybox |
| `api::kernel_satp::{get, set}` | task runtime、trap |
| `api::user_aspace_lifecycle` | task exit |
| `api::mempolicy` | syscall get_mempolicy |

## `mm::frame_alloctor`

| 符号 | 说明 |
|------|------|
| `PhysicalFrameAllocator` | trait（来自 api-v0） |
| `GlobalPhysFrameAllocator` | 零大小适配器，委托全局栈分配器 |
| `init_frame_allocator` | bring-up 初始化帧池 |
| `frame_alloc_result` / `frame_dealloc_result` | 分配/回收 |
| `frame_inc_ref` / `frame_ref_count` | COW 引用计数 |
| `frame_mem_stats` | 内存池统计 |

## `mm::kernel_mm`

### 类型与常量

- `LoadElfError`, `LoadProgramError`, `LoadedElf`, `PrepareUserStackError`
- `RootVolumeReadError`, `ExecResolveError`
- `DEFAULT_USER_ELF_PATH`

### 函数（按 feature）

**`impl-sv39` 或 `impl-loongarch64` 启用时：**

| 函数 | 说明 |
|------|------|
| `init(start_ppn, end_ppn, ram_end_exclusive)` | 内核页表与帧池 |
| `kernel_satp()` | 当前内核地址空间 token |
| `from_elf_path` / `from_elf_bytes` | 装载用户 ELF |
| `load_program_from_path` | ELF + shebang 统一入口 |
| `prepare_elf_user_stack` | 写用户栈，返回 sp |
| `fork_user_aspace` | fork，返回 `(aspace_ptr, token)` |
| `drop_user_aspace` | 释放用户地址空间 |
| `handle_cow_fault` | 写时复制 |
| `handle_user_page_fault` | 惰性/栈/brk 缺页 |
| `madvise_discard_pages` | 丢弃已映射页 |
| `map_identity_range_user` / `map_anon_range_user` | 测试/特殊映射 |
| `ensure_user_execute_for_kernel_va` | 内核 VA 补 X 权限 |

**仅 `impl-dummy`（无平台 impl）时：**

- `init` / `map_*` 为空操作；`from_elf_path` 返回 `BadClass`；`fork_user_aspace` 返回 `Unsupported`。

## `mm::user_aspace`

- `with_user_aspace_mut(handle, f)`：在 `LoadedElf::user_aspace_ptr` 上执行闭包（实现 `HeapBrk` / `MmapOps`）。

## `mm::user_access`

| 类型 | arch |
|------|------|
| `Sv39UserMemoryOps` | RISC-V |
| `LoongArch64UserMemoryOps` | LoongArch64 |
| `debug_probe_user_virt`, `UserVirtProbe` | 仅 Sv39（诊断） |

## `mm::mempolicy`

| 函数 | 说明 |
|------|------|
| `is_user_addr_mapped` | `MPOL_F_ADDR` 路径校验 |
| `get_mempolicy_single_node` | 单节点 get 逻辑 |
| `fill_get_mempolicy_nodemask` | 写 nodemask |

## 未通过聚合层导出的内容

以下 **故意不** 从 `wateros-mm` 根模块暴露：

- `pagetable::*`、PTE 编码、walk 实现细节
- `impl-common` 内部辅助
- 各 `impl-*` crate 根模块（仅经 `kernel_mm` 子集转发）

依赖方应使用 `mm::api` 契约与 `mm::kernel_mm` 聚合 API，而非直接依赖 `wateros-mm-impl-sv39` 等。
