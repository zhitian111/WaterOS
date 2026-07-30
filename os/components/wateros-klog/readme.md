# wateros-klog

`wateros-klog` 是 WaterOS 的内核消息环。它提供无分配的内核记录入口、固定上限的 ring-buffer
存储，以及 `syslog(2)`/`dmesg` 所需的内核侧读写语义。它不是 runtime console logger：console
输出、日志过滤和用户地址拷贝由其他模块负责。

## 分层与边界

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合 | `src/lib.rs` | 版本化 API、当前实现、稳定宏和兼容转发；不保存状态。 |
| API | `klog-api/api-v0/` | 记录头、flags、syslog action、ring trait 和统计快照。 |
| 实现 | `klog-impl/impl-kernel/` | `global.rs` 全局服务、ringbuf 槽、IRQ guard、无分配格式化与 syslog 缓冲语义。 |
| ABI 调用方 | `wateros-syscall/.../sys/misc/syslog.rs` | 拷贝用户缓冲、校验长度、将返回值映射为 syscall 结果。 |

```text
klog_info! / record
  -> record_fmt（512 B 栈缓冲，无分配）
  -> record_with_meta（填 timestamp / caller_id）
  -> KlogRingbuf::with（关本 CPU 中断 + 全局 Mutex）
  -> KlogRingbufInner::append（覆盖写 + sequence）

sys_syslog
  -> syscall 层复制用户缓冲
  -> klog::syscall::dispatch_kernel
  -> ring 读游标 / traditional `<level>text\n` 格式
```

## 核心数据

| 数据 | 位置 | 含义 |
| --- | --- | --- |
| `KlogRecordMeta` | API | sequence、单调时间、facility、level、flag、caller ID 与正文长度。 |
| `Slot` | `state.rs` | 一条有效记录和固定上限正文。 |
| `KlogRingbufInner` | `state.rs` | 私有的 `slots`、下一写槽 `head`、可见条数、sequence 与全局 read cursor。 |
| `KLOG` | `global.rs` | `Mutex<Option<KlogRingbufInner>>`；首次访问惰性初始化。 |
| `KlogFmtBuffer` | `impl-kernel/src/format.rs` | 私有的 512 B 栈缓冲，供日志宏避免热路径分配。 |

环满时，新记录覆盖最旧槽并增加 `records_dropped`。若全局 read cursor 正指向被覆盖记录，
它会推进到下一 sequence；随后再根据 `oldest_seq` 夹紧，避免 READ 永远指向不可见记录。
同一记录正文超过 `KLOG_MAX_RECORD_BYTES` 时设置 `KlogFlags::TRUNC`。

## 锁、中断与重入

`KlogRingbuf::with` 的临界区顺序是：保存中断状态 → 屏蔽当前 CPU 全局中断 → 获取 `KLOG`
自旋锁 → 操作 ring → 释放锁 → 恢复原中断状态。

这样避免同一 CPU 在持 ring 锁时被中断，而中断处理又尝试记录日志导致自旋死锁。该锁不替代
其他子系统的锁，也不保护 console、VFS、MM、scheduler 或用户内存。

**最重要的规则：**ringbuf 内部操作及 syslog 的单条格式化都在 klog 锁内执行。内部回调不得：

- 调用 `klog_*`、`record`、`stats` 或任何可能再次记录日志的函数；
- 调度、等待、执行用户内存访问，或取得会由日志路径再取得的锁；
- 长时间操作或把 `KlogRecordView.text` 保存到锁外。

需要转发到 console/网络或做复杂格式化时，应先在锁外准备输出缓冲，再短暂读取；当前 API 的
借用 view 不适合直接在锁外保留。

## syslog 行为与限制

- `READ` 读取下一未读记录但不推进 cursor；`READ_CLEAR` 读取并推进；`READ_ALL` 连续读取并
  逐条推进；`CLEAR` 将 cursor 移到最新记录之后。
- read cursor 是**全局**的，不按进程或 fd 分开；多个调用者会互相消费未读记录。
- READ 输出 traditional 行 `<level>text\n`；它不把内部元数据裸露给用户态。
- WRITE priority 的高 3 位为 level、低 3 位为 facility，且写入 `USER` flag。
- `CONSOLE_ON/OFF/LEVEL` 当前是兼容占位，不改变 runtime console；未知 action 仍会触发 panic，
  因此 syscall 层应只传经 ABI 校验的 type。

## 验证与排障

```sh
make -C os rv_check
make -C os la_check
```

ringbuf 单元测试覆盖 append、unread cursor 和基本 read 行为。排查日志丢失时读取 `stats()`：
`records_dropped` 增长表示 descriptor 覆盖；`oldest_seq`、`newest_seq` 与 `read_cursor_seq`
可用于判断消费者是否落后于环容量。
