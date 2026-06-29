# 系统快照（当前）

归纳自各组件 `docs/exports/features/*.md` 与根 `os/Cargo.toml`。日期：2026-06-29。

## 定位

WaterOS 0.1.0 是 **QEMU bring-up 内核**：组件化、`api-v0` + `impl-*` 可替换，目标是在 RISC-V（OpenSBI）与 LoongArch64（virt）上跑 busybox、基础 ELF 测程与 LTP 子集，而非生产级通用 OS。

## 平台与构建

| 项 | 状态 |
|----|------|
| 默认 feature | `qemu-riscv64-opensbi` |
| 第二主线 | `qemu-loongarch64-virt` |
| 页表 | RISC-V Sv39 / LoongArch64 三级 4KiB 页 |
| 块设备 | RISC-V virtio-mmio；LoongArch virtio-pci |
| 控制台 | OpenSBI（RISC-V）/ 16550 MMIO（LoongArch） |
| 根 FS | ext4 RW（`ext4_rs`，经 vfs bridge + 页缓存） |

## 启动链路（摘要）

`kernel_main`：runtime/platform 初始化 → MM/任务/trap → 驱动 `init_after_boot` → `fs::init`（探测不挂载）→ 用户 bring-up 总线（RW 挂根卷、跑测程）→ 定时器 → `task::run_first_task`。

## 各层能力一览

### 地基

- **base / base-config**：地址 newtype、单核 `UniprocessorSafeCell`、堆/任务/FS/IPC 常量。无 feature 分叉。
- **utils**：仅占位 `add()`，主线几乎未用。
- **abi**：Linux generic64 调用号表；errno、参数包、返回值编码。两平台共用一张表。
- **platform**：trap/切换/分页/定时器/串口/关机；arch 与板级 impl 二选一。
- **runtime**：panic、控制台、`log` 桥、TLSF/链表堆；与 klog 分工明确。

### 执行与系统调用

- **mm**：ELF 装载、brk/mmap、COW fork、用户缺页、软件 walk 拷贝；帧分配器为栈式 LIFO。
- **task**：内核/用户任务、fork/clone/exec、wait 族、多类调度骨架（bring-up 以 SCHED_OTHER 为主）；单核假设。
- **syscall**：`impl-kernel` 覆盖文件、进程、内存、信号、futex、poll/epoll、INET/UNIX socket、shm 等 broad 子集；未实现槽位 bring-up 期可 panic。
- **cred**：per-task 侧表，root 初始；set*id 可用；cap/inode 权限多为桩。

### I/O 栈

- **driver**：virtio blk/net、块缓存、UART、smoltcp；DTB/PCI 探测；devfs 刷新。
- **fs**：ext4 RW、devfs、procfs、根卷/辅助挂载；`FsAsyncIo` 未实现。
- **vfs**：路径解析、挂载命名空间、fd 表、cwd、页缓存叠加；经 bridge 消费 fs。
- **ipc**：waitqueue、ringbuf pipe、futex、SysV shm 子集、信号状态机；顶层 api-v0 仍占位。
- **klog**：环缓冲 + `syslog` syscall；无 `/dev/kmsg`；权限未校验。

## 已验证用途（bring-up）

- QEMU 上挂载 ext4 根分区，运行 glibc/musl basic ELF 与 busybox 脚本。
- LTP 并行跑测（feature `bringup-ltp-*`）；网络 iperf/netperf 依赖 smoltcp。
- 双架构 syscall/trap 路径贯通；LoongArch 与 RISC-V 共用 generic64 ABI 表。

## 共性缺口

- **单核**：无 SMP、无真实 IRQ 驱动链。
- **安全**：凭证与 VFS open 未深度联动；klog 任意读写。
- **语义**：大量 Linux 兼容为最小子集（sched RT、NUMA、完整 procfs、SUID exec 等）。
- **骨架**：ipc/api-v0、utils、ipc-event、部分 dummy impl 仅占编译位。

## 延伸阅读

- 组件细节：[`../features/`](../features/)
- 关系与接线：[`../architecture/`](../architecture/)
- 阶段表述：[`../release-overview/current.md`](../release-overview/current.md)
