# wateros-klog — 聚合层公共 API

## 用途

内核与其它组件（`wateros-syscall` 等）通过 `klog` crate 使用的真实导出接口。

## 再导出

| 路径 | 来源 |
|------|------|
| `klog::api` | `wateros-klog-api-v0` 全部公共项 |
| `klog::KlogRingbuf` | `wateros-klog-impl-ringbuf` |

## 聚合层函数

| 函数 | 说明 |
|------|------|
| `init()` | 清空全局环 |
| `post_init_hello()` | 写入固定 boot 问候记录 |
| `record(level, facility, text)` | 追加记录（自动时间戳与 caller_id） |
| `record_with_meta(meta, text)` | 使用调用方已填 meta |
| `stats()` | `KlogStats` 快照 |
| `iter_from(start_seq, f)` | 按序号升序回调 `KlogRecordView` |
| `ts_nsec_now()` | 单调时钟纳秒 |
| `caller_id_now()` | 当前任务 ID 或 0 |

## 宏（`#[macro_export]`）

`klog_trace!` / `klog_debug!` / `klog_info!` / `klog_warn!` / `klog_error!` — `format_args!` → `record`（facility 固定 `LOG_KERN`）。

## 类型

| 类型 | 说明 |
|------|------|
| `KlogFmtBuffer` | 宏用 512 字节栈缓冲；`new` / `as_bytes` |

## 子模块

### `klog::export`

| 函数 | 说明 |
|------|------|
| `format_traditional(meta, text, out) -> usize` | 传统 `"<N>...\n"` 格式化 |

### `klog::syscall`

| 函数 | 说明 |
|------|------|
| `dispatch_kernel(action, user_buf, user_len) -> isize` | `sys_syslog` 内核语义；缓冲由 syscall 层提供 |

## `klog::api`（api-v0 摘要）

| 类别 | 主要符号 |
|------|----------|
| 元数据 | `KlogRecordMeta`, `KlogFlags`, `KlogStats`, `KlogRecordView` |
| 常量 | `LOG_KERN`, `LOG_USER`, `LOG_EMERG`…`LOG_DEBUG` |
| syslog action | `SYSLOG_ACTION_*`, `decode_action`, `is_write_priority` |
| trait | `KlogStore` |
| 错误 | `KlogError`, `AppendResult` |

## `KlogRingbuf`（impl）

| 方法 | 说明 |
|------|------|
| `init()` | 全局环初始化 |
| `with(f)` | 持锁访问 `KlogRingbufInner` |
| `iter_from(start_seq, f)` | 持锁迭代 |

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出 |
