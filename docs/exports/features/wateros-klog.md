# wateros-klog 功能快照

## 用途

记录 **`wateros-klog`** 一级组件的能力边界、与 **`wateros-runtime-logging`** / **`syslog(2)`** 的分工，以及实现状态。完整设计见 **[`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)**。

## 事实来源

- 设计基线：[`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)
- 计划路径：`os/components/wateros-klog/`（**尚未创建**）
- 关联：`wateros-abi`（`SYSLOG = 116`）、`wateros-syscall`（`sys_syslog`）、`wateros-base-config`（环容量常量）

## 实现状态

| 项 | 状态 |
|----|------|
| 组件目录与 Cargo workspace 成员 | **已实现**（`os/components/wateros-klog/`） |
| `klog-api` / `klog-ringbuf` | **已实现** |
| `sys_syslog` 接线 | **已实现**（`__NR_syslog` = 116） |
| 根 `wateros` | `klog::init()` 早于 `runtime::logging::init()` |
| 设计文档 | [docs/architecture/wateros-klog.md](../../architecture/wateros-klog.md) |

## 设计目标（已评审）

- 内核**可查询**消息环：固定 `KlogRecordMeta` + 变长正文，desc + text ring 存储。
- **不**依赖 `log!` crate；自有 `klog_*!` / `klog::record`。
- 用户态经 **`__NR_syslog` (116)** 以**传统 ASCII 线**读取；WRITE 与内核写入**同一环**。
- bring-up **权限全开**；测试期未支持路径 **panic** 以驱动补全。

## 计划子 crate

| 子 crate | 职责 |
|----------|------|
| `klog-api/api-v0` | `KlogRecordMeta`、`KlogFlags`、`KlogStore` trait、`SyslogAction`、错误类型 |
| `klog-impl/klog-ringbuf` | 全局环、spin 锁、`append` / `iter` / read cursor |
| `wateros-klog` 聚合 `src/lib.rs` | `init`、宏、`export`、`syscall::dispatch` 再导出 |

## 配置（计划，`wateros-base-config`）

- `KLOG_DESC_SLOTS`（默认 256）
- `KLOG_TEXT_RING_BYTES`（默认 32 KiB）
- `KLOG_MAX_RECORD_BYTES`（默认 1024）

## 明确未覆盖（首期设计外）

- `/dev/kmsg` 行协议导出。
- Linux `printk_ringbuffer` 字典环、`dev_printk_info`。
- `CAP_SYSLOG` / uid 校验。
- 与 `runtime-console` 的 `CONSOLE_ON/OFF/LEVEL` 硬联动（列入 P3）。
- SMP 无锁 prb 对标实现。

## 维护要求

代码落地或 syscall 行为变化时，同步更新本文件、[`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md) 与 [`docs/exports/public-api/wateros-klog.md`](../public-api/wateros-klog.md)。
