# RIO-04：VFS 读取租约与普通文件实现

## 任务目标

在 VFS API 中定义统一的“准备读取、锁外复制、按复制进度提交/回滚”契约，并首先为
普通文件和 procfs 文件实现。该契约是 pipe、socket、eventfd 和设备适配的共同前置，
必须先以独立 API 提交冻结。

## 前置条件

- RIO-02 已提供部分 user-copy 进度。
- RIO-03 已让 dup/fork 共享 OFD offset/status，并允许在 OFD 中保存 reservation。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/exports/impl-guide/wateros-vfs.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/read-family/02-user-copy-progress.md`
- `docs/tasks/read-family/03-open-file-description-state.md`

## 已知信息与代码证据

当前 syscall 先让句柄推进 offset，再执行用户拷贝：

```rust
let n = read_fd(fd, &mut kbuf)?;
match copy_to_user(ptr, &kbuf[..n]) {
    Ok(w) if w == n => success(n),
    _ => error(EFAULT),
}
```

`BufferedFileHandle::read()` 和 `PagedFileHandle::read()` 都在返回前执行：

```rust
self.offset = self.offset.checked_add(n as u64)?;
```

因此无效用户地址会导致 `EFAULT`，但 offset 已经前进。`read_at()` 已存在且不改变
顺序 offset，可以作为普通文件租约的 staging 原语。

## 涉及文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- `os/components/wateros-vfs/src/fd.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/proc_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/dir_handle.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs`

## 任务内容

先在 `api-v0` 建立统一的准备、读取和提交协议，再完成普通文件、
procfs 和目录路径的接入。接口名称可以按现有风格调整，但必须保留下述语义。

### 建议 API

以下为契约示例，允许按项目命名调整，但语义不能减少。prepared read 用于跨越 fd
短锁，lease 表示 source 已经保留并 staging 的数据：

```rust
pub struct VfsCopyProgress {
    pub copied: usize,
    pub complete: bool,
}

pub enum VfsReadFinish {
    Bytes(usize),
    Fault,
}

pub trait VfsReadLease: Send {
    fn bytes(&self) -> &[u8];

    /// 根据用户拷贝结果提交；消耗 self 后必须解除 reservation。
    fn finish(
        self: Box<Self>,
        progress: VfsCopyProgress,
    ) -> VfsResult<VfsReadFinish>;
}

pub trait VfsPreparedRead: Send {
    /// 可等待数据、执行 ext4/network I/O；调用时已不持 fd/OFD spin lock。
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>>;
}

pub trait VfsIoHandle {
    /// 只能短时捕获 Arc 状态并登记 reservation，不能等待或做慢 I/O。
    fn prepare_read(&mut self, max_len: usize)
        -> VfsResult<Box<dyn VfsPreparedRead>>;
}
```

必须满足：

- prepare 成功后，相关 OFD 上的下一次顺序 read/lseek/使用当前 offset 的 write 不得
  越过 reservation。
- `prepare_read()` 的 trait 默认实现只能返回 `Unsupported`；不能默认调用旧的破坏性
  `read()`，否则遗漏适配的句柄会静默保留 R4 问题。
- prepared object 只能持有 owned/`Arc` 状态，不能借用 fd 表内 boxed handle。
- `acquire()`、user-copy 和 `finish()` 调用时均不持 `SharedIoHandle.inner`。
- `bytes()` 是内核拥有的稳定 staging 数据，不引用临时锁 guard。
- `finish` 只提交实际到达用户空间的字节。
- lease 未 `finish` 就 Drop 时自动 cancel、offset 不变并唤醒 waiter。
- 创建失败、EOF 和 copy fault 都必须解除 reservation。
- 不在持 OFD、页缓存、ext4 或地址空间自旋锁时调用 user-copy。

## 普通文件实现

建议状态：

```rust
enum ReadReservation {
    Idle,
    Active { id: u64, offset: u64, staged: usize },
}
```

流程：

1. 短锁检查并登记 reservation，捕获当前共享 offset。
2. `prepare_read()` 克隆路径和 `Arc<FileDescriptionState>`，返回 owned prepared object。
3. 离开 fd lock 后，`acquire()` 使用 `read_at(offset, staging)`；失败则取消
   reservation。
4. syscall 调用 `copy_to_user_progress()`。
5. `finish` 短锁验证 reservation id。
6. 完整或部分复制时 offset 仅增加 `copied`；零字节 fault 时 offset 不变。
7. 清除 reservation 并唤醒等待同一 OFD 的操作。

不能在 `finish` 时重新读文件路径，也不能通过 `seek(-n, SEEK_CUR)` 回滚已经推进的
offset；并发 lseek/write 会使这种回滚不可靠。

## procfs 与目录

- 有顺序 offset 的 procfs 文件使用同一 lease 契约；动态内容应在 acquire 时生成快照，
  lease 生命周期内保持稳定。
- 目录 `prepare_read` 返回 `NotAFile`，由 RIO-01 映射 `EISDIR`。
- stateless EOF 节点可以返回空 lease，不得使用 `Unsupported` 伪装 EOF。

## 内存和上限

- staging 分配必须使用 fallible allocation。
- fd/access/type 校验必须发生在 staging 分配之前。
- 一次 lease 上限使用内部实现上限 `SYSCALL_IO_MAX`，大用户 count 通过合法短读处理。
- `MAX_RW_COUNT` 是 ABI 上限，`SYSCALL_IO_MAX` 是内核资源上限，两者不可混用。
- 不在 VFS API 中依赖 syscall crate 常量；将合理的 VFS staging 上限从调用方传入。

## 并行与提交

先提交 `api-v0` 契约和默认实现，再提交普通文件/proc 实现，最后提交 `sys_read` 接入。
RIO-05 至 RIO-08 只能基于已合入的 API 提交开发，不能各自定义不兼容 lease。

## 如何验收

组件测试：

- full copy 后 offset 增加 `n`；
- 跨页 fault 且复制 `k > 0` 时仅增加 `k`；
- 首字节 fault 时 offset 不变；
- lease Drop/cancel 后 offset 不变；
- 两个 dup fd 不会越过 active reservation；
- 独立 open 不互相阻塞；
- EOF 返回 0；
- staging OOM 不改变 offset。

guest 对照：

```text
memfd/file 写入 "xyz"
offset=0
read(fd, invalid_ptr, 3) -> EFAULT
lseek(fd, 0, SEEK_CUR)   -> 0
read(fd, valid, 3)       -> "xyz"
```

执行：

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
```

最终压力回归由 RIO-10 完成。

## 搜索范围与交付

用 `rg "fn read\\(|fn read_at|fn seek|with_current_io|with_io"` 审核 VFS API、fd-session、
fs-bridge 和 syscall read 路径。所有仍可被 syscall 访问却未实现 `prepare_read` 的
句柄必须列入 RIO-05 至 RIO-08，不能依赖默认 fallback。

按“API 契约、普通文件/proc impl、syscall 接入”拆分提交。组件测试放对应 impl；
诊断日志放 `/tmp`。完成后在索引勾选 RIO-04，并记录 lease Drop、partial commit、
OFD spin-lock 外执行的证据。

## 禁止做法

- 不持 spin mutex 跨 user-copy/page fault。
- 不持 fd/OFD spin mutex 跨 `acquire()`、ext4 I/O 或 waitqueue sleep。
- 不用预检查地址替代 lease。
- 不在 EFAULT 后盲目 seek 回退。
- 不让 lease Drop 静默遗留 reservation。
- 不为未适配句柄提供“调用旧 read”的兼容 fallback。
