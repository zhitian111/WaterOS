# wateros-klog 公共 API 快照

## 用途

描述 **`wateros-klog`** 聚合 crate 对内核其它模块暴露的稳定入口。组件已实现于 `os/components/wateros-klog/`；下列接口以 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md) 与源码 rustdoc 为准。

## 事实来源

- [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)
- 计划：`os/components/wateros-klog/src/lib.rs`

## 计划聚合层导出

| 符号 | 说明 |
|------|------|
| **`init()`** | 初始化全局 `KlogRingbuf`（清空）；须在 `runtime::logging::init()` **之前**调用。 |
| **`post_init_hello()`** | 内核主线初始化完成后写入 `hello wateros\n`（`main` 在 `run_first_task` 前调用）。 |
| **`record(level, facility, text)`** | 底层写入一条记录；返回 `AppendResult`（成功序号或错误）。 |
| **`klog_trace!` / `klog_debug!` / `klog_info!` / `klog_warn!` / `klog_error!`** | 宏层：`format_args!` → `record`；**不**转发 `log!`。 |
| **`stats()`** | 返回 `KlogStats` 快照。 |
| **`iter_from(seq)`** | 内核侧从序号起迭代记录（可见性 / 自检）。 |
| **`global()`** | 访问全局 `KlogStore`（具体类型以实现为准）。 |

## 计划子模块（聚合 `pub mod`）

| 模块 | 说明 |
|------|------|
| **`export`** | `format_traditional(meta, text) -> [u8]` 等；供 syscall 与 future `/dev/kmsg` 复用。 |
| **`syscall`** | `dispatch(action, buf, len) -> isize`；仅 `wateros-syscall` 应直接依赖。 |

## `klog-api` 契约类型（计划）

| 类型 / trait | 说明 |
|--------------|------|
| **`KlogRecordMeta`** | `repr(C)` 记录头；内核可读，不导出给用户态裸结构。 |
| **`KlogFlags`** | `CONT` / `TRUNC` / `USER` 等。 |
| **`KlogLevel` / `KlogFacility`** | 与 syslog 优先级对齐。 |
| **`KlogStats`** | 提交/丢弃计数与序号范围。 |
| **`KlogStore`** | 环抽象：`append`、`iter_from`、`unread_bytes`、`buffer_bytes`、读游标推进。 |
| **`SyslogAction`** | 与 Linux `SYSLOG_ACTION_*` 常量对齐。 |
| **`AppendResult` / `KlogError`** | 写入与查询错误。 |

## Feature 与依赖（计划）

| 项 | 说明 |
|----|------|
| 根 `wateros` | 新增 `klog` path 依赖；`qemu-riscv64-opensbi` 等主线 feature 启用。 |
| `wateros-syscall` | `impl-kernel` 依赖 `wateros-klog`。 |
| `wateros-runtime` | **无**对 klog 的依赖。 |

## 缺口说明

- 存储为 desc 槽 + 每槽上限 `KLOG_MAX_RECORD_BYTES` 正文（非独立 byte-ring 碎片管理）。
- `CONSOLE_ON/OFF/LEVEL` 首期 no-op；未接 `runtime-console`。
- 测试期未知 syslog action **panic**。

## 维护要求

新增 `pub` 项或变更 `init` 顺序时，同步更新本文件与 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)。
