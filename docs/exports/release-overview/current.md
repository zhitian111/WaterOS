# 阶段版本概述

**版本**：wateros 0.1.0  
**阶段**：双架构 QEMU bring-up（2026 年中）  
**事实来源**：`os/Cargo.toml`、`os/feature-tree.txt`、各组件 `docs/exports/features/*.md`

---

## 这版是什么

WaterOS 0.1.0 不是可安装的发行版，而是一颗 **可替换实现的教学/实验内核**：代码按一级组件拆分，用 Cargo feature 在编译期选定 trap、页表、驱动和文件系统后端。当前精力集中在 **让真实用户程序在 QEMU 上跑起来**——挂载 ext4 根分区、执行 busybox 与 LTP 子集，并同时维护 **RISC-V（OpenSBI）** 与 **LoongArch64（virt）** 两条主线。

## 已经能做什么

**启动与基础设施**  
内核能在 QEMU virt 上完成自举：解析 DTB、初始化串口与内核堆、建立 Sv39 或 LoongArch 页表，并把 trap 接到统一的 syscall 分发器。panic 会打印信息后关机；开发日志走 `log` crate，内核消息另有一条 klog 环可供 `syslog` 读取。

**进程与系统调用**  
支持多任务调度（以 CFS 类 SCHED_OTHER 为主）、fork/clone/exec、wait 族、信号与 futex、管道与 poll/epoll。syscall 面覆盖大量 Linux 64 位通用调用号：文件读写与路径操作、内存 mmap/brk、凭证查询、INET/UNIX socket（smoltcp 后端）、SysV shm 子集等。bring-up 策略下，遇到未实现的 syscall 可能直接 panic，便于尽早暴露缺口。

**存储与设备**  
块设备经 VirtIO 暴露（RISC-V 用 mmio，LoongArch 用 PCI），可选写穿块缓存。根文件系统为 **可读写 ext4**；另有 devfs、procfs 及挂载命名空间（bind、tmpfs 伪 FS 等）。VFS 层提供 per-task 文件描述符表与当前工作目录。

**验证场景**  
默认构建可在 QEMU 上跑用户 bring-up 总线：先 RW 挂载根卷，再按阶段执行 glibc/musl 基础 ELF、busybox 脚本；支持 LTP 定向跑测与网络工具（iperf/netperf）所需的 socket 子集。

## 刻意没做或只做桩的

- **多核**：全局单核假设，无 SMP 调度与 IPI。
- **安全模型**：凭证侧表存在，但 open 权限、capabilities、SUID exec 等多为最小语义或 TODO。
- **完整 Linux 兼容**：sched RT、完整 procfs、NUMA、异步 I/O、完整 xattr/ACL 等按测程需要逐步补齐。
- **生产驱动**：中断未挂 PLIC/APLIC 完整链；非 QEMU 平台仅有 dummy impl。
- **骨架组件**：`wateros-utils`、ipc 顶层 api-v0 等占位，不代表稳定对外 API。

## 适用对象

| 适合 | 不适合 |
|------|--------|
| 学习组件化内核结构 | 需要开箱即用桌面/服务器 OS |
| 在 QEMU 上扩展 syscall/驱动 | 多核生产部署 |
| 对照 Linux 行为做 LTP/ busybox 回归 | 指望完整 POSIX 或安全认证 |

## 如何构建与选型

- 默认：`cargo build` 等价于 **RISC-V + OpenSBI + Sv39** 全套 feature。
- LoongArch：`--features qemu-loongarch64-virt`。
- feature 树全貌：`os/feature-tree.txt`；组件能力与缺口见 [`../features/`](../features/)。

## 下一步（从缺口归纳，非承诺）

权限与 VFS 打通、LoongArch/RISC-V 专用 syscall 号表、SMP 与中断、弱化 bring-up panic 策略、补齐 ipc-event 与异步 I/O。具体排期见 `docs/roadmap/`。
