# wateros-klog

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-klog` 是 WaterOS 的内核消息环。它提供无分配的内核记录入口、固定上限的 ring-buffer
存储，以及 `syslog(2)`/`dmesg` 所需的内核侧读写语义。它不是 runtime console logger：console
输出、日志过滤和用户地址拷贝由其他模块负责。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 版本化 API、当前实现、稳定宏和兼容转发；不保存状态。 |
| klog API | `klog-api/api-v0/` | 记录头、flags、syslog action、ring trait 与统计快照。 |
| 内核实现 | `klog-impl/impl-kernel/` | `global.rs` 全局服务、ringbuf 槽、IRQ guard、无分配格式化与 syslog 缓冲语义。 |
| ABI 调用方 | `wateros-syscall/.../sys/misc/syslog.rs` | 拷贝用户缓冲、校验长度、将返回值映射为 syscall 结果。 |

## 实现说明

- 记录路径无分配：`record_fmt` 使用 512 B 栈缓冲（`KlogFmtBuffer`），避免热路径分配。
- 环满时新记录覆盖最旧槽并增加 `records_dropped`；若全局 read cursor 正指向被覆盖记录，它
  会推进到下一 sequence，再按 `oldest_seq` 夹紧，避免 READ 永远指向不可见记录。
- 单条记录正文超过 `KLOG_MAX_RECORD_BYTES` 时设置 `KlogFlags::TRUNC`。
- 锁与中断：`KlogRingbuf::with` 临界区顺序为保存中断状态 → 屏蔽当前 CPU 全局中断 → 获取
  `KLOG` 自旋锁 → 操作 ring → 释放锁 → 恢复原中断状态。避免同一 CPU 持 ring 锁时被中断，而
  中断处理又尝试记录导致自旋死锁。
- **最重要的规则**：ringbuf 内部操作及 syslog 单条格式化都在 klog 锁内执行。内部回调不得：
  调用 `klog_*`/`record`/`stats` 或任何可能再次记录日志的函数；调度、等待、执行用户内存访问
  或取得会由日志路径再取得的锁；长时间操作或把 `KlogRecordView.text` 保存到锁外。
- read cursor 是**全局**的，不按进程或 fd 分开；多个调用者会互相消费未读记录。
- READ 输出 traditional 行 `<level>text\n`，不把内部元数据裸露给用户态；WRITE priority 高 3
  位为 level、低 3 位为 facility，并写入 `USER` flag。
- `CONSOLE_ON/OFF/LEVEL` 当前是兼容占位，不改变 runtime console；未知 action 仍会触发 panic。

## 调用链路

记录路径：

```text
klog_info! / record
  -> record_fmt（512 B 栈缓冲，无分配）
  -> record_with_meta（填 timestamp / caller_id）
  -> KlogRingbuf::with（关本 CPU 中断 + 全局 Mutex）
  -> KlogRingbufInner::append（覆盖写 + sequence）
```

读取路径：

```text
sys_syslog
  -> syscall 层复制用户缓冲
  -> klog::syscall::dispatch_kernel
  -> ring 读游标 / traditional `<level>text\n` 格式
```

## 各实现功能

### klog-api / 记录 API

API 契约与数据模型的主要实现在 `klog-api/api-v0/src/`。

- `KlogRecordMeta`：单条记录头，字段为 `seq`（单调递增）、`ts_nsec`（单调时间）、`text_len`、
  `facility`、`level`、`flags` 与 `caller_id`；`append` 会覆盖 `seq`、`text_len`，并在正文超限
  时写入 `KlogFlags::TRUNC`。
- `KlogFlags`：位标志，`TRUNC`（正文被截断）、`USER`（`syslog(2)` WRITE 来源）等。
- syslog action 常量：`READ` / `READ_CLEAR` / `READ_ALL` / `CLEAR` / `WRITE` /
  `CONSOLE_ON/OFF/LEVEL` / `SIZE_BUFFER` / `SIZE_UNREAD` / `OPEN` / `CLOSE`。
- `KlogStore` trait：环的读写契约（`append` / `peek_next_unread` / `advance_read_cursor` /
  `clear_read_cursor` / `unread_bytes` / `buffer_bytes` / `stats`）。
- `KlogStats`：`records_committed` / `records_dropped` / `oldest_seq` / `newest_seq` /
  `read_cursor_seq`，供 `stats()` 与排障使用。

### impl-kernel / 内核实现

主要实现在 `klog-impl/impl-kernel/src/`。

`state.rs`——固定槽环与覆盖策略：

- `Slot { valid, meta, bytes: [u8; KLOG_MAX_RECORD_BYTES] }`：一个 descriptor 槽，正文为固定
  上限内联数组，追加不分配。
- `KlogRingbufInner`：`slots: [Slot; KLOG_DESC_SLOTS]`、`head`（下一写槽）、`count`（可见条数）、
  `next_seq`、`oldest_seq`、`read_cursor_seq`、`records_committed`、`records_dropped`。
- `append` 在满环时覆盖 `head` 槽：`records_dropped` 加一；若 `read_cursor_seq == dropped_seq`，
  先推进到 `dropped_seq + 1`，随后由 `refresh_oldest_seq` 按 `oldest_seq` 夹紧，保证 READ 不会
  永久指向已丢失记录。
- `slot_index_for_seq` / `for_each_valid_seq` 从 `head` 倒序扫描有效槽，避免遍历整个数组。

`global.rs`——全局实例与中断安全锁：

- `static KLOG: debug::TrackedMutex<Option<KlogRingbufInner>>`：首次访问由 `ensure_inner` 惰性
  初始化。
- `KlogRingbuf::with` 临界区顺序：`KlogInterruptGuard::new()`（保存中断状态 → 关本 CPU 全局
  中断）→ `KLOG.lock()` → 闭包 → 解锁 → guard `Drop` 恢复原中断状态；防止同 CPU 中断日志重入
  自旋锁死锁。
- `record(level, facility, text)` 自动填 `ts_nsec_now()`（平台时间不可用时返回 0）与
  `caller_id_now()`（无调度上下文时返回 0）；`stats()` 返回锁内复制的快照；`init()` 清空全局环。

`format.rs`——无分配格式化：

- `KlogFmtBuffer { buf: [u8; 512], len }`：实现 `core::fmt::Write`，正文超过容量静默截断，日志
  路径不分配也不 panic。
- `record_fmt(level, args)`：把 `format_args!` 写入 512 B 栈缓冲后调用 `record`。
- `format_traditional`：输出 `<level>text\n`，缓冲不足时截断并尽量以 `\n` 结尾。

`syslog.rs`——syslog 内核缓冲语义：

- `dispatch_kernel(action, kernel_buf, kernel_len)`：`WRITE` priority 高 3 位 level、低 3 位
  facility；`READ*` 使用 `KERNEL_LINE_MAX = 2048` 栈行缓冲，在**同一个 ring 锁闭包内**完成
  peek、格式化与 cursor 推进，避免读到已被覆盖的 text；未知 action 直接 panic。
- `read_one` 按 `advance` 决定是否推进 cursor；`read_all` 循环读取直到缓冲满或 `NoUnread`，
  截断时只拷入剩余容量并停止。

## 验证与排障

```sh
make -C os rv_check
make -C os la_check
```

ringbuf 单元测试覆盖 append、unread cursor 和基本 read 行为。排查日志丢失时读取 `stats()`：
`records_dropped` 增长表示 descriptor 覆盖；`oldest_seq`、`newest_seq` 与 `read_cursor_seq`
可用于判断消费者是否落后于环容量。
