# wateros-klog — 已实现功能快照

## 用途

记录内核消息环（klog）当前能力、syscall 覆盖与缺口。设计基线见 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md)。

## 子 crate

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-klog-api-v0` | `KlogRecordMeta`、`KlogStore` trait、syslog action 常量 | 已实现 |
| `wateros-klog-impl-ringbuf` | desc 槽 + 每槽变长正文环、全局 `Mutex` + 中断屏蔽 | 已实现 |
| `wateros-klog`（聚合） | `init`、`record`、`klog_*!` 宏、`export`、`syscall` | 已实现 |

## 存储模型

- **desc 槽**：`KLOG_DESC_SLOTS`（`base-config`），满时覆盖最旧，`records_dropped` 递增。
- **正文**：每槽最多 `KLOG_MAX_RECORD_BYTES`；`SIZE_BUFFER` 返回 `KLOG_TEXT_RING_BYTES`。
- **读游标**：`CLEAR` / `READ_CLEAR` 仅推进游标，物理记录保留供 `iter_from`。

## 已实现能力

- **内核写入**：`record` / `record_with_meta`；`klog_trace!` … `klog_error!`（512 字节栈缓冲）。
- **观测**：`stats`、`iter_from`；`post_init_hello` 写入 boot 问候行。
- **时间/任务**：`ts_nsec_now`（`platform::timer`）、`caller_id_now`（`task::current_task_id`）。
- **用户态导出**：`format_traditional` → `"<N>...\n"` 线格式。
- **sys_syslog**（`dispatch_kernel`）：
  - `CLOSE`/`OPEN`：no-op
  - `SIZE_UNREAD` / `SIZE_BUFFER`
  - `READ` / `READ_CLEAR` / `READ_ALL`
  - `CLEAR`：mark 全部已读
  - `CONSOLE_OFF`/`ON`/`LEVEL`：no-op（未联动 runtime 控制台）
  - WRITE（priority 编码）：写入同一环，`USER` 标志
  - 未知 action：**panic**

## Feature（聚合层）

| Feature | 说明 |
|---------|------|
| `default` | `api-v0` + `impl-ringbuf` + `platform-timer` + `task-api` |
| `platform-timer` | 时间戳来源 |
| `task-api` | `caller_id` 来源 |

## 权限与安全

bring-up 阶段：**不检查** uid / `CAP_SYSLOG`；任意进程可 READ/WRITE/CLEAR。

## 缺口与后续

- `CONSOLE_*` action 未实现真实控制台策略。
- 无 `/dev/kmsg` 设备节点（仅 syscall 读路径）。
- 环满覆盖后 `iter` 与 `READ` 对 gap 的处理以实现 rustdoc 为准。
- 测试期未知 action panic；稳定后可改为 errno + `klog-strict-panic` feature。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
