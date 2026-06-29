# wateros-cred — 阶段版本概述

## 适用范围

支撑 musl/glibc/BusyBox 对 **UID/GID 系统调用** 的自检与基本 `set*id` 行为，服务于 rootfs bring-up。**不是**多用户安全产品。

## 本阶段已具备

- 独立一级组件，api/impl 分离，与 VFS per-task 资源（cwd、fd、mount_ns）模式一致。
- 用户任务默认 **root（0）**；syscall 层可更新当前任务八 ID 与 supplementary 组。
- fork 继承、线程共享、reap 清理闭环；`getgroups(0)` 等 LTP 常见路径可用。

## 本阶段刻意简化

- **Privileged set*id**：未实现 Linux 权限拒绝与 saved UID 复杂规则的全部边角。
- **AccessCheck**：`may_access_inode` 恒 true；capability 仅 `Chown`/`SysAdmin` 枚举占位。
- **VFS stat** 与 ext4 inode owner 未完全打通；exec setuid 位未生效。

## 下一阶段方向

- `on_exec` + ELF 权限位；`faccessat` 使用真实 inode owner。
- 引入非 root impl 或 capability 位图；`prctl(PR_CAPBSET_*)` 归口 cred。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版 |
