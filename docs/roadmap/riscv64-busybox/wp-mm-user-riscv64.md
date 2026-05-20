# 工作包：wateros-mm — RISC-V64 用户地址空间与 brk/mmap 第一版

**所属**：`os/components/wateros-mm`（及与 `wateros-task` 中用户任务装载的边界契约）。  
**并行度**：可与 **平台/驱动脚手架**、**VFS fd 设计** 并行起步；**syscall 真实现**强依赖本包可联调语义。

## 要做什么

1. 在 **Sv39** 默认路径上，为 **单个用户任务** 建立可用的用户虚拟地址区间契约（与 `spawn_user_task` / ELF loader 输出对齐）：代码段、数据、BSS、用户栈的映射与权限。
2. 将当前 `wateros-syscall` 中 **`brk` 原子桩** 的替换点落实为可调用的 **MM 原语**：在任务携带 **`LoadedElf::user_aspace_ptr`**（Sv39 用户页表）时经 **`mm::user_aspace::with_user_aspace_mut`** 调用 **`HeapBrk`/`MmapOps`**；否则仍使用假顶桩（如 LoongArch 或未走 ELF 装载的用户任务）。**`brk` 与匿名 `mmap`** 协同策略：**堆**仅通过 **`brk`** 在 **`[brk_start, brk_max)`** 内饥渴扩页；**匿名私有映射**通过 **`mmap`** 自 **`mmap_arena_base`** bump 分配（详见 `docs/exports/features/wateros-mm.md`）。
3. **`mmap`/`munmap`/`mprotect` 第一版**：满足后续 `execve` 装载与简单 `mmap(MAP_ANONYMOUS)` 测例；错误路径返回 Linux 风格 errno 映射（经 `wateros-abi`）。
4. 与 **trap 页错误** 策略对齐：采用 **饥渴（eager）映射**（见 `impl-sv39` `pagetable` 模块说明）；用户访问已 **`munmap`** 的 VA 在 U 态触发 **page fault**。
5. **Trap / `satp`（与 syscall 联调）**：用户 ELF 装载仅 **`map_kernel_ram_identity`**（不含 MMIO）；来自用户的 trap 在 **`wateros_kernel_trap_handler`** 内切 **内核 `satp`** 再跑 syscall/FS，**`sret` 前** 装回用户 **`satp`**（见 `task::trap_runtime`、`os/src/trap_handler.rs`）。避免在用户表嵌 MMIO 或与 **`mmap` @ `0x10000000`** 抢 VA 的权宜方案。

## 验收要求

- [x] 用户镜像地址空间在日志中可验证：至少输出 **image 区间、栈顶、初始 brk 或 mmap 基址** 之一（由 bring-up 总线阶段触发，见 `wp-init-test-bus.md`）。
- [x] 同一用户任务内连续 `brk(0)` 返回值稳定；`brk` 非法收缩返回 `EINVAL`（或与选定策略一致）且在总线日志可观察。
- [x] `mmap`/`munmap` 最小闭环：映射一页、写入用户 VA、unmap 后再次访问应 **trap 为可识别错误** 或 **syscall 层不可达**（策略与架构组一致即可，但须文档化）。

## 验证方式

1. **`stage-02-mm`**（`os/src/user_bringup_mm.rs`）：根卷存在 **`/glibc/basic/brk`**、**`mmap`**、**`munmap`** 等测程（与 `os/tem/glibc/basic` 等镜像产物一致）时，`from_elf_path` + `spawn_user_task_from_loaded_elf`；串口日志 **`[mm-bringup]`** 含 `loaded`/`spawned` 行，缺文件为 **`skip`**。
2. QEMU 跑一轮，**`grep '\[mm-bringup\]'`** 对照路径与任务号；测程实际 syscall 行为在 **`run_first_task`** 后与 `self_tests` 并行调度中执行。
3. bring-up 或调试路径可通过 **`mm::user_aspace::with_user_aspace_mut`** 与 **`api::HeapBrk`/`MmapOps`** 直接操作页表指针（与 syscall 拼合路径一致）。
4. VFS 路径：**`write` → `munmap` → `close(fd)`** 后无内核 **StorePageFault @ `0x10001050`**（MMIO 区）；确认 handler 内已切内核 **`satp`**。

## 依赖

- **上游**：`wateros-platform` arch 初始化、内核页表已建立。
- **下游**：`wp-vfs-fd-session.md`、`wp-syscall-mem-time.md`、`wp-syscall-process-exec.md`。

## 可并行对象

`wp-platform-driver-scaffold.md`；VFS fd 的 **API 设计**（不写实现）可并行。
