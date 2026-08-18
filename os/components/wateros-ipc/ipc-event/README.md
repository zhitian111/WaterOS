# ipc-event 预留模块离线开发手册

`ipc-event` 当前只是一个未接线的 crate 边界：源码只有测试用 `add`，没有 event 对象、等待
队列、句柄、registry 或 syscall 语义。它不在 `wateros-ipc` workspace members 中，也不是聚合
crate 的依赖/feature，因此正常 WaterOS 构建不会编译或重导出它。整体 IPC 现状见
[wateros-ipc](../README.md)。

实际 Linux `eventfd` 当前实现位于
[`sys/ipc/eventfd.rs`](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs)，作为
VFS I/O handle 持有 `Arc<EventFdState>`。不要误把本 crate 当成 eventfd 已模块化完成。

## 当前 eventfd 的真实数据结构

虽然不属于本 crate，迁移时必须保留这些语义：

```text
EventFdState
  inner: Mutex<EventFdInner>
    counter: u64（最大可用值 u64::MAX - 1）
    nonblocking: bool
    next_read_id
    read_reservation: Option<{id, value}>
  wait: WaitQueue

EventFdHandle
  state: Arc<EventFdState>
  semaphore: bool
```

读不是直接减 counter，而是三阶段：

```text
reserve_read
→ 复制 8 字节到用户空间
→ finish_read(commit=true)

用户 copy 失败或 lease Drop
→ finish/cancel，不消费 counter
```

普通模式 reservation value 为全部 counter，semaphore 模式为 1。单一
`read_reservation` 会串行并发 reader，防止多个 reader 同时预留同一计数。write 解析精确 8
字节，拒绝 `u64::MAX` 和会溢出上限的加法；阻塞/非阻塞、poll readiness 和 wake 都通过 VFS
handle + waitqueue 完成。

## 为什么不能直接“移动文件”

eventfd 横跨三个边界：

- IPC 层：counter、reservation、wake 条件；
- VFS 层：open description、dup/fork、read/write/poll/close handle；
- syscall 层：`eventfd2` flags、fd 安装、8 字节用户 copy、errno。

若把整个 `EventFdHandle` 搬入 IPC，会让 IPC 依赖 VFS；若只搬 counter 却删除 reservation，会
使 EFAULT 偷消费计数。因此应先抽取纯领域对象 API，再保留 VFS adapter。

## 建议的目标分层

```text
ipc-event API:
  EventCounter、EventMode、EventError
  reserve_read / commit_read / cancel_read
  try_write / readable / writable

ipc-event impl:
  Mutex<EventInner> + WaitQueue
  reservation ID 与 counter 不变量

VFS adapter:
  Arc<EventCounter>、OFD flags、poll、read/write lease

syscall:
  eventfd2 flags、fd 分配、用户 copy、ErrNo
```

IPC API 不应接收用户指针或返回 `ErrNo`；VFS adapter 不应重新实现计数规则；syscall 不应直接
锁内部 state。

## 迁入 WaterOS IPC 的实施步骤

1. 将本 crate 加入 `components/wateros-ipc/Cargo.toml` workspace。
2. 建议拆成 `event-api/api-v0` 与 `event-impl/impl-core`，或至少先明确公共/实现模块边界。
3. 从现有 eventfd 源码提取纯状态机，先用相同测试证明行为等价。
4. event impl 依赖 `ipc-waitqueue`，对象锁释放后再 wake/wait；禁止持 state 锁进入 scheduler。
5. 顶层增加可选 `event` dependency、feature 和 `pub mod event`，`all/self_test` 同步。
6. syscall 的 `EventFdHandle` 改为薄 VFS adapter，仍负责 prepared-read lease 和 copy 事务。
7. 删除本 crate 的 `add` 模板和假单测。
8. 更新 [wateros-ipc README](../README.md) 中“ipc-event 未接线”的说明。

迁移应分阶段，每一步保证旧 syscall 仍可构建；不要一次改对象、VFS、syscall、feature 后只靠
编译定位语义回归。

## 状态机不变量

- `counter <= u64::MAX - 1`，输入 `u64::MAX` 永远无效。
- 同一 reservation 只能 commit/cancel 一次，错误 ID 不得消费新 reservation。
- EFAULT、取消、被 signal 打断不会偷减 counter。
- 普通读成功原子取走全部；semaphore 读成功只减 1。
- overflow write 在 blocking 模式等待可写，在 nonblocking 模式返回 WouldBlock/EAGAIN。
- 状态改变后先释放 mutex，再唤醒相关 waiters/poll observers。
- 最后 handle 关闭唤醒阻塞者，并定义其关闭结果；不能永久睡眠。

## 测试矩阵

| 类别 | 场景 |
| --- | --- |
| 创建 | initial 0、非零、非法 flags、CLOEXEC/NONBLOCK/SEMAPHORE |
| read | 精确 8 字节、短 buffer、counter 0、普通/sem 模式 |
| write | 0、1、`u64::MAX`、接近上限、overflow 阻塞/非阻塞 |
| 事务 | copy fault 后计数不变、lease Drop cancel、错误 reservation ID |
| 并发 | 多 reader、多 writer、read reservation 与 write 竞争 |
| fd | dup/fork 共享对象、CLOEXEC、close 最后引用 |
| poll | 0→可读、接近上限→不可写、read/write 后 readiness 更新 |
| 打断 | signal、timeout（若接口支持）、任务退出清理 |

## 与其它“event”概念的区分

- epoll event 是就绪通知 ABI，状态在 syscall epoll 实现；
- input event 是设备输入记录，状态在 GUI/VFS input handle；
- inotify event 是文件监控队列；
- signal 是异步任务通知；
- waitqueue 是调度阻塞机制；
- eventfd 是可读写的 64 位计数对象。

不要因为名字相同就把它们合并进通用 `Event` enum；它们的所有权、队列、ABI 和溢出语义完全
不同。

## 当前验证与接线检查

```bash
cd os
cargo test --manifest-path components/wateros-ipc/ipc-event/Cargo.toml
rg -n "ipc-event|pub mod event|dep:event" components/wateros-ipc
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

当前第一条只测试占位 `add`，没有功能证明力。只有完成 facade feature、VFS adapter 和用户态
eventfd/poll/dup/fault 测试后，才能把本 crate 标记为已接入。
