# wateros-klog — 版本概述

## 当前阶段目标

让内核与用户态工具（busybox、`dmesg`）能通过 **Linux 116 号 `syslog`** 读取内核消息环，同时供内核模块以结构化方式观测环内容与统计。

## 已具备的能力

- 内核 `klog_info!` 等宏与 `record` API 写入全局环。
- 用户态 `sys_syslog`：`READ` / `READ_CLEAR` / `READ_ALL` / `CLEAR` / `SIZE_*` / WRITE（priority）等主要 action。
- 读出的线为传统 `"<level>message\n"` 格式，兼容常见测例期望。
- Boot 后 `post_init_hello` 提供非空 `dmesg` 基线。
- 环满时覆盖最旧记录并统计 `records_dropped`。

## 适用范围

- **适用**：启用 `dep:klog` + `klog/default` 的 QEMU 主线（RISC-V / LoongArch feature 树均已接线）。
- **限制**：`CONSOLE_*` syslog action 尚未改变控制台行为；权限检查在 bring-up 阶段全开。
- **与开发日志**：`log::info!` 仅控制台；进环须显式 `klog_*!`。

## 演进方向（非当前承诺）

- 与 `runtime-console` 联动 `CONSOLE_LEVEL`。
- `/dev/kmsg` 或 cred 权限收紧。
- 可选 kmsg 线格式与更贴近 Linux `printk_ringbuffer` 的存储布局。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
