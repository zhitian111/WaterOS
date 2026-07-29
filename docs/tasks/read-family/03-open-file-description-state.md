# RIO-03：共享 open-file-description 状态

## 任务目标

修正 `dup/dup2/dup3/fcntl(F_DUPFD)/fork` 后文件偏移和 status flags 被复制而不是共享的
问题，为读取租约提供稳定的共享状态。descriptor flags（如 `FD_CLOEXEC`）继续保持
每个 fd slot 独立。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/exports/impl-guide/wateros-vfs.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/exports/impl-guide/wateros-ipc.md`
- `docs/tasks/read-family/README.md`

## 已知信息与代码证据

`SharedIoHandle::duplicate()` 当前调用具体句柄 `duplicate()`，再创建新的
`OpenFileDescription`：

```rust
let duplicate = inner.handle.duplicate()?;
Ok(Self::new(duplicate))
```

`BufferedFileHandle` 和 `PagedFileHandle` 的 `duplicate()`/`Clone` 会复制 `offset`。
fork 的 `fd_table_copy_plan()` 同样对每个 slot 调用 `handle.duplicate()`。

这违反 Linux OFD 语义：

- 同一 open 得到的 dup/fork fd 应共享当前文件偏移；
- `F_SETFL` 修改的 `O_NONBLOCK/O_APPEND` 应在 dup fd 上可见；
- `FD_CLOEXEC` 属于 fd descriptor，本来就应该独立；
- 对同一路径分别执行两次 open，偏移必须独立。

pipe 的 `PipeEndpoint` 通过 `Cell<bool>` 保存 `nonblocking/direct`，clone 后同样不共享
status flags。

另一个直接相关问题是：

```rust
pub fn with_io<R>(&self, f: impl FnOnce(&mut dyn VfsIoHandle) -> VfsResult<R>) {
    let mut inner = self.inner.lock();
    f(inner.handle.as_mut())
}
```

`inner` 是 `spin::Mutex`，而具体 `read()` 可能进入 pipe waitqueue 或 socket polling。
同一 fd 被多个线程使用时，其它 CPU 会在该锁上长期自旋。RIO-04 必须在短锁内取得
owned prepared operation，不能继续把整个阻塞 read 放在 `with_io` 闭包中。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- `os/components/wateros-vfs/src/fd.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/endpoint.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/handles.rs`
- `os/components/wateros-driver/driver-network/src/socket_handles.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dup.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fcntl.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/clone.rs`

## 任务内容

不要简单让所有 dup fd 共享当前 `SharedIoHandle.inner` 自旋锁。当前具体 `read()` 可能
阻塞，直接共享会让另一个 fd 在同一 spin mutex 上长期自旋，并可能重新引入此前的
并发 fd 阻塞问题。

推荐把“fd slot 的短时操作保护”和“Linux OFD 共享状态”分开：

```rust
struct FileDescriptionState {
    offset: Mutex<u64>,
    status_flags: AtomicU32,
    // RIO-04 后加入 read reservation / generation。
}

struct PagedFileHandle {
    path: String,
    description: Arc<FileDescriptionState>,
    // backing resource state...
}
```

具体句柄 `duplicate()` 生成独立 wrapper/操作锁，但 clone 同一个
`Arc<FileDescriptionState>`。普通文件、pipe endpoint、Unix/inet socket 都要审计其
status flags 和底层资源是否已经是共享状态。

所有可阻塞读取源还必须能从短锁内导出拥有稳定 `Arc` 状态的 prepared read 对象。该
对象离开 `with_current_io` 后仍安全，但不能保留借用自 boxed handle 的引用。

关闭语义必须基于最后一个 OFD 引用：

- 关闭一个 dup fd 不能关闭 pipe endpoint 或 socket；
- 最后引用关闭时执行一次底层 close/flush；
- fork 子进程退出不能提前影响父进程；
- close error 不得让 fd slot 复活。

## 必须一起完成的内容

1. 普通文件 offset 共享。
2. `O_APPEND/O_NONBLOCK` 等 status flags 共享。
3. pipe endpoint status flags 共享。
4. fork 与所有 dup 入口使用同一语义。
5. descriptor flags 保持 per-slot。
6. 分别 open 同一路径仍创建独立 OFD。
7. 为 RIO-04 预留读事务状态，但本任务不实现事务算法。

只修 `dup()` 而不修 fork，或者只修文件 offset 而让 `F_SETFL` 继续复制，都不能验收。

## 锁与生命周期

- status flag 可使用原子；offset 和事务状态使用短时 SMP 锁。
- 不在持 OFD 状态锁时进入 ext4、pipe wait、network poll 或 user-copy。
- 不在持 `SharedIoHandle.inner` 的 spin mutex 时等待数据、执行 ext4/network I/O 或
  user-copy。
- 具体 read 先短锁取得/保留 offset，再锁外执行慢操作，最后短锁提交；完整协议由
  RIO-04 实现。
- 保持现有 lock ordering 文档，新增顺序必须写在相关模块头注释。
- 不用 inode 作为 OFD key；两个独立 open 即使 inode 相同也不能共享 offset。

## 并行与边界

本任务可与 RIO-01、RIO-02 并行，但会与 RIO-04 修改相同文件。先合入本任务，再让
RIO-04 基于共享状态增加 reservation。

不在本任务改变文件系统 page cache、ext4 写回或 scheduler。

## 如何验收

组件测试和 guest 测试至少覆盖：

```text
open -> dup -> read(a,1)="a" -> read(b,1)="b"
open -> fork -> 父子顺序读取共享 offset
dup 后 F_SETFL(O_NONBLOCK) 在另一 fd 的 F_GETFL 可见
FD_CLOEXEC 在 dup fd 上按 Linux 规则独立
两次独立 open 同一路径，各自第一次读取 "a"
关闭一个 dup pipe/socket fd 后另一 fd 仍可用
最后一个引用关闭时底层 close 只执行一次
同一 fd 一个线程阻塞 read 时，另一线程不会在 fd spin mutex 上无限自旋
```

执行：

```bash
cd os
make rv_check
make la_check
```

运行时还需通过 RIO-10 的多线程共享 fd 压测。任何任务双跑、spin lock 长期占用或
close 后 UAF 都视为失败。

## 搜索范围与交付

用 `rg "duplicate\\(|fd_table_copy_plan|open_status_flags|open_accmode|offset"` 审核全部
`VfsIoHandle` 实现、dup/fcntl/clone 入口和 concrete handle clone。特别检查
descriptor flag 与 status flag 是否被错误放在同一层。

本任务可与 RIO-01、RIO-02 并行，但 RIO-04 必须等待本任务 API/状态提交。测试写在
`impl-fd-session` 及相关 concrete impl；日志放 `/tmp`。完成后在索引勾选 RIO-03，
记录 offset/status/close 测试和锁序说明。
