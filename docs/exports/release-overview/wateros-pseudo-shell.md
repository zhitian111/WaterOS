# wateros-pseudo-shell — 阶段版本概述

## 适用范围

内核仍缺少可靠用户态 init 或网络 shell 时，在 **QEMU 串口** 上提供最小文件系统与 ELF 执行验证。面向开发者 smoke test，不面向终端用户。

## 本阶段已具备

- 阻塞式 `wateros>` 提示符与 `cd`/`ls`/`stat`/`rm`/`exec`/`help`。
- 与 VFS 组合 API（`root::read_view`、`mount::open_rw_session`）对齐真实 syscall 路径的数据面。
- RISC-V 下 `exec` 完整走通：装载、栈、spawn、cred/vfs hook、wait/reap。

## 本阶段刻意简化

- 单线程内核任务内 REPL；无 job control。
- 非 RISC-V 架构 `exec` 不可用。
- 错误信息为 `Debug` 格式，非 POSIX errno 文案。

## 典型使用场景

1. 验证 ext4 根卷挂载后可列举、删除文件。
2. 在引入 BusyBox 前手动 `exec` 测小程序。
3. CI/本地 bring-up 日志中确认 VFS + task + mm 链路。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版 |
