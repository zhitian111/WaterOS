# wateros-syscall — 阶段版本概述

## 适用范围

面向 BusyBox / LTP / musl-glibc 用户态 bring-up 的 **Linux generic 64-bit syscall 兼容层**。当前主线目标：在 QEMU virt（RISC-V、LoongArch）上跑通交互式 shell 与大批量测例，而非生产级安全内核。

## 本阶段已具备

- **完整分发骨架**：号表解码、O(1) 热路径分发、trap/信号/定时器协作接口。
- **广覆盖 syscall 实现**：文件、进程、内存、信号、futex、poll/epoll、IPv4/UNIX 网络、凭证 get/set、klog syslog 等，足以支撑当前 rootfs 与 LTP 子集。
- **可观测性**：未实现槽位在 bring-up 配置下主动 panic，便于测例驱动补全；线程 bring-up 统计与 LTP fast-exit 辅助降低 CI 挂死。

## 本阶段刻意简化

- 权限与 capability 多经 `wateros-cred` impl-root 放行；VFS open 等路径未必校验 owner。
- 部分 ioctl、控制终端、时间戳持久化、journal 级 rename 等为最小语义。
- 双架构共用同一号表；平台差异仅限 trap 寄存器约定。

## 下一阶段方向

- 收紧 `dispatch_unsupported` 策略（部分槽位由 panic 改为 `-ENOSYS` 或完整实现）。
- cred/VFS 联动真实 inode uid/gid 与 `faccessat`。
- execve S_ISUID/S_ISGID 与 `cred::on_exec` 打通。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版 |
