# 工作包：wateros-mm — RISC-V64 用户地址空间与 brk/mmap 第一版

**所属**：`os/components/wateros-mm`（及与 `wateros-task` 中用户任务装载的边界契约）。  
**并行度**：可与 **平台/驱动脚手架**、**VFS fd 设计** 并行起步；**syscall 真实现**强依赖本包可联调语义。

## 要做什么

1. 在 **Sv39** 默认路径上，为 **单个用户任务** 建立可用的用户虚拟地址区间契约（与 `spawn_user_task` / ELF loader 输出对齐）：代码段、数据、BSS、用户栈的映射与权限。
2. 将当前 `wateros-syscall` 中 **`brk` 原子桩** 的替换点落实为可调用的 **MM API**：至少支持 libc 常见的 `brk(0)` 查询与有限扩展，或与 **匿名 `mmap`** 协同的明确策略（二选一须在 `docs/exports/features/wateros-mm.md` 或 crate rustdoc 写清）。
3. **`mmap`/`munmap`/`mprotect` 第一版**：满足后续 `execve` 装载与简单 `mmap(MAP_ANONYMOUS)` 测例；错误路径返回 Linux 风格 errno 映射（经 `wateros-abi`）。
4. 与 **trap 页错误** 策略对齐：若采用 eager 映射，文档说明；若采用 demand paging，需定义最小可测行为。

## 验收要求

- [ ] 用户镜像地址空间在日志中可验证：至少输出 **image 区间、栈顶、初始 brk 或 mmap 基址** 之一（由 bring-up 总线阶段触发，见 `wp-init-test-bus.md`）。
- [ ] 同一用户任务内连续 `brk(0)` 返回值稳定；`brk` 非法收缩返回 `EINVAL`（或与选定策略一致）且在总线日志可观察。
- [ ] `mmap`/`munmap` 最小闭环：映射一页、写入用户 VA、unmap 后再次访问应 **trap 为可识别错误** 或 **syscall 层不可达**（策略与架构组一致即可，但须文档化）。

## 验证方式

1. 在 **`wateros-mm`** 或 **`wateros`** bring-up 总线中增加 **`mm::user_bringup::test()`**（名称可自定），仅在 `qemu-riscv64-opensbi` 下编译；内部使用 **内核已知 VA 或临时测试映射**，不依赖完整 open(2)。
2. QEMU 跑一轮，日志中 grep `mm.*bringup`（或工作包约定前缀），对照验收清单勾选。
3. 与用户 ELF 联调时：在 `wp-syscall-process-exec` 未完成前，可用 **内核侧直接调用** `UserMemoryOps` 模拟读写，避免与 syscall 环耦合调试。

## 依赖

- **上游**：`wateros-platform` arch 初始化、内核页表已建立。
- **下游**：`wp-vfs-fd-session.md`、`wp-syscall-mem-time.md`、`wp-syscall-process-exec.md`。

## 可并行对象

`wp-platform-driver-scaffold.md`；VFS fd 的 **API 设计**（不写实现）可并行。
