# RIO-07：eventfd 原子读取提交

## 任务目标

让 eventfd 的 8 字节固定宽度读取在 `EFAULT`、signal、dup/fork 和并发 read/write 下
保持计数器原子性：只有完整复制 8 字节后才减少 counter。

## 前置条件

- RIO-02 user-copy 进度已合入。
- RIO-03 OFD 生命周期已合入。
- RIO-04 读取租约 API 已合入。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/read-family/04-vfs-read-lease-and-files.md`

## 已知信息与代码证据

当前 `EventFdHandle::read()` 在写入内核 buffer 前已经减少 counter：

```rust
let value = if self.semaphore { 1 } else { inner.counter };
inner.counter -= value;
buf[..8].copy_from_slice(&value.to_ne_bytes());
```

随后 syscall 的 user-copy 若失败，counter 已永久改变。eventfd 与普通 stream 不同：
读取必须恰好传输 8 字节，不能把跨页 fault 后的前几个字节当作一次成功读取。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- eventfd 相关 dispatch/测试文件

## 任务内容

在 `EventFdState` 中加入 read reservation：

```rust
struct EventReadReservation {
    id: u64,
    value: u64,
}
```

流程：

1. `buf_len < 8` 继续返回 `EINVAL`，不等待、不改 counter。
2. counter 为 0 时按 blocking/nonblocking 规则等待或返回 `EAGAIN`。
3. 短锁捕获待读 value 并登记 reservation，但不减少 counter。
4. 锁外把 8 字节 staging 写入用户空间。
5. 只有 `copied == 8 && complete` 时提交：
   - semaphore 模式减 1；
   - 普通模式减去 reservation 捕获的 value。
6. 任意 partial/zero fault 都取消 reservation、counter 不变并返回 `EFAULT`。
7. 并发 write 可增加 counter；commit 只减捕获值，不能覆盖后来的加法。
8. commit/cancel 唤醒等待 reader/writer。

同一 eventfd 上第二个 reader 在 reservation 完成前必须等待，不能读取同一 counter
快照。等待期间 signal 中断不影响 reservation owner。

## 生命周期

- dup/fork 共享同一个 `EventFdState`。
- lease Drop 或 owner task exit 自动取消 reservation。
- close 最后一个引用时唤醒 waiter，不能留下悬挂 token。
- 不在持 eventfd spin lock 时进行 user-copy 或任务 sleep。

## 如何验收

- `read(fd, buf_len=7)` 返回 `EINVAL`，counter 不变。
- 首字节 EFAULT 返回 `EFAULT`，下一次 valid read 得到原值。
- 跨页只复制部分 8 字节仍返回 `EFAULT`，counter 不变。
- 普通模式并发 write 发生在 lease 期间，commit 后保留新增值。
- semaphore 模式每次完整 read 只减 1。
- nonblocking counter=0 返回 `EAGAIN`。
- signal interrupt 未取得值时返回 `EINTR`。
- dup/fork reader 不会取得同一值两次。

执行：

```bash
cd os
make rv_check
make la_check
```

优先复用仓库 LTP eventfd 用例，并在 RIO-10 增加 invalid/cross-page buffer 定向测试。

## 搜索范围、并行与交付

用 `rg "EventFd|eventfd|counter|semaphore"` 审核 eventfd create/read/write/poll、
dup/fork 和 close 路径，确认没有第二个绕过 lease 的 counter 消费入口。

本任务可与 RIO-05、RIO-06、RIO-08 并行。测试放 eventfd 模块或聚合自测，日志放
`/tmp`。完成后在索引勾选 RIO-07，记录 normal/semaphore、partial fault、并发 write
和生命周期结果。
