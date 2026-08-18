# Per-task FD Session 离线开发手册

[VFS 总览](../../README.md) · [VFS API](../../vfs-api/api-v0/README.md) · [FS Bridge](../impl-fs-bridge/README.md) · [Pipe](../../../wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/README.md) · [TTY](../../../wateros-tty/tty-impl/impl-console/README.md)

本 crate 实现“已经打开的资源如何成为 fd”：每任务 descriptor table、共享 open-file description、cwd/root/exec 元数据、POSIX record lock/flock，以及 console、字符设备、pipe/FIFO、socketpair、PTY 和可选用户图形设备的 `VfsIoHandle`。

路径查找和普通文件创建由 FS bridge 完成；syscall 层负责 Linux 参数、用户 copy 和 errno；底层 pipe/TTY/driver 负责缓冲状态机。这里的核心责任是共享关系、锁边界和最后引用释放。

## 1. 源码地图

| 文件 | 职责 |
|---|---|
| `registry.rs` | fd table、owner/refcount、OFD 包装、dup/fork/exec/exit |
| `cwd.rs` | cwd/root、exe path、argv/env/auxv 与 `CLONE_FS` 共享 |
| `file_lock.rs` | 每 inode POSIX byte-range lock 与 flock |
| `handles.rs` | console、null/zero/random、pipe/FIFO、Unix stream pair |
| `char_dev_handle.rs` | driver character device、console TTY、RTC、prepared read |
| `pty.rs` | `/dev/ptmx`、`/dev/pts/N`、`/dev/tty` 句柄适配 |
| `user_graphics.rs` | feature-gated framebuffer/evdev 用户设备 |
| `interrupt_guard.rs` | 在 irq-disabled 临界区执行闭包的薄封装 |
| `lib.rs` | 特殊设备注册/探测和 self-test 聚合 |

## 2. fd、slot、OFD 与底层对象

必须区分四层：

```text
进程/线程的整数 fd
  -> FdSlot（descriptor flags：CLOEXEC/O_PATH）
  -> SharedIoHandle
  -> OpenFileDescription（handle + closed）
  -> 具体 VfsIoHandle（文件、pipe endpoint、PTY、设备……）
```

### 2.1 `OpenFileDescription`

```rust
handle: Box<dyn VfsIoHandle>
closed: bool
```

`close_once()` 先置 `closed=true`，再调用 handle.close，保证即使后端 close 返回错误也不会重复关闭。Drop 再调用一次但忽略结果。因此显式 close 是向用户报告 writeback/设备错误的唯一可靠位置。

### 2.2 `SharedIoHandle`

关键字段：

- `inner: Arc<Mutex<OpenFileDescription>>`：实际 live OFD；
- `snapshot`：安装时通过 handle.duplicate 取得的备用描述；
- `terminal_id/resource_kind`：安装时缓存的不可变分类，close-on-exec 不必等待阻塞 I/O。

`with_io` 在 OFD mutex 下运行整个回调，可能长时间阻塞；调用方必须先从全局 fd registry 克隆 `SharedIoHandle`，释放 registry 锁后再调用。

`prepare_read` 只在 OFD 锁下创建独立 reservation，随后设备等待和用户 copy 不持 registry/OFD 锁。

`duplicate()`：

1. live inner 可 `try_lock` 时调用 live handle.duplicate；
2. live I/O 正在持锁时，改从安装时 snapshot duplicate；
3. snapshot 不存在则返回 Busy，不能无锁读 live handle。

这个 snapshot 是可用性折衷：若 concrete handle 的可变状态没有通过内部 `Arc` 共享，而是只存在对象字段中，旧 snapshot 可能落后。新增 handle 时必须明确 duplicate 后哪些状态共享、哪些独立。

### 2.3 `FdSlot`

`flags` 当前：

- `FD_CLOEXEC=1`：descriptor flag；
- `FD_PATH_ONLY=2`：O_PATH，只允许路径/metadata 类操作。

这些不是 OFD status flags。O_APPEND/O_NONBLOCK/O_SYNC 等由 concrete handle 的 `open_status_flags` 管理，dup/fork 后按 OFD 语义共享。

slot 还缓存 `resource_kind` 和 PTY `terminal_id`，用于无需进入阻塞 handle 的分类、hangup 与 close-on-exec 编排。

## 3. `PerTaskFdRegistry` 数据结构

```text
tables:      owner TaskId -> Arc<Vec<Option<FdSlot>>>
owners:      member TaskId -> owner TaskId
ref_counts:  owner TaskId -> 共享 fd-table 的任务数
open_counts: owner TaskId -> 已占用 slot 数
free_fds:    owner TaskId -> BTreeSet<空洞 fd>
```

`open_counts/free_fds` 是索引，不是真实所有权；任何批量替换 table 后必须 `rebuild_table_indexes`。

### 3.1 惰性初始化

`ensure_task` 为新 owner 创建表并安装：

```text
fd 0 -> 默认 serial stdin 或 ConsoleInHandle
fd 1 -> 默认 serial stdout 或 ConsoleOutHandle
fd 2 -> 独立默认 stdout handle
首个动态 fd = VFS_FIRST_DYNAMIC_FD
```

如果任务创建 hook 遗漏，首次 fd syscall 会兜底生成标准 fd，但不会自动继承父资源。因此 fork/clone 必须使用显式 snapshot/share 两阶段 hook。

VFS 聚合层的全局 `FD_REGISTRY` 与 `CWD_REGISTRY` 当前都是 `MaybeUninit + READY Atomic` 的裸惰性初始化，没有 once 状态机；首次访问必须在串行 bring-up 完成。多 CPU 同时首次调用可能并发写静态存储。扩展时应采用统一 once-cell。

### 3.2 fd 分配

- 先检查当前 task 的 `RLIMIT_NOFILE` 和 `open_counts`；
- 有空洞时从 `free_fds.range(minfd..)` 取最低 fd；
- 无空洞时扩展 Vec；
- `dup3(newfd)` 若目标已打开，先从表中取出 displaced handle，安装新 slot 后由调用方在 registry 锁外关闭 displaced；
- `newfd >= rlimit`：dup3 返回 BadFd；普通 dup/minfd 返回 TooManyOpenFiles。

表扩容、BTreeMap/BTreeSet 插入和很多 `Vec` 操作当前是不可失败分配，内核堆 OOM 会 panic。增加批量 fd API 时必须增加显式上限和 fallible allocation，而不能只依赖 RLIMIT 的数值检查。

## 4. fork、CLONE_FILES、unshare、exec、exit

### 4.1 普通 fork

高性能路径：

```text
registry lock
  -> fd_table_fork_snapshot(parent)：克隆 Arc<Vec<FdSlot>>，O(1)
release lock
  -> child task/MM/其它资源准备
registry lock
  -> install_fd_table_fork_snapshot(child)
  -> child 自己成为 owner，table Arc 与 parent 暂时相同
后续任一 descriptor-table 修改
  -> Arc::make_mut：复制 slot Vec
  -> slot 中 SharedIoHandle 仍共享同一 OFD
```

因此父子 close/dup/CLOEXEC 修改各自表，不互相删 slot；但文件 offset、pipe endpoint、O_APPEND 等 OFD 状态仍共享，符合 fork 语义。

旧的 `fd_table_copy_snapshot/install_fd_table_copy` 是两阶段逐槽安装入口；调用 handle duplicate 必须在 registry 锁外。

### 4.2 `CLONE_FILES`

`share_fd_table_from_parent` 让 child 的 owners 指向同一 owner并增加 refcount。descriptor table 修改立即对所有线程可见，包括 close、dup 和 FD_CLOEXEC。

### 4.3 `close_range(CLOSE_RANGE_UNSHARE)`

若 refcount>1：

1. 复制原 table；
2. 对每个 slot 调用 `FdSlot::duplicate`；
3. caller 是 owner 时，把其余共享者 re-home 到某个 sibling；
4. caller 安装独立表并重建索引。

当前每槽 duplicate 失败用 `.ok()` 静默转换为 `None`，即该 fd 在新表中被关闭，而整个 `unshare_fd_table()` 仍返回成功。这是明确的兼容性/可靠性缺口；严格实现应先全部 fallible duplicate 到临时表，任一失败则不发布新 owner/table。

### 4.4 exec

正确链路应先在 registry 锁内 `take_cloexec_fds_for_task`，再在锁外逐个：

- 处理 PTY/终端 hangup 所需信息；
- 释放 record/flock 状态；
- 调用 `SharedIoHandle::close`。

不能持 registry 锁调用可能写回、睡眠或进入 driver 的 close。只有 exec 成功提交时才应永久关闭 CLOEXEC；exec 回滚边界由 syscall/task 层定义。

### 4.5 exit/reap

`drain_task_fd_table`/`drop_task_fd_table` 先解除 member→owner：

- shared table 仍有成员：当前 task 不遍历/关闭 slot；
- 最后成员：`Arc::try_unwrap` 成功后取出每个 handle；
- 具体 close 应尽量在全局 registry 锁外完成；
- POSIX process locks 还需在进程最后线程退出时调用 `release_process_all_locks`，不能只等 parent reap。

## 5. close 与并发 I/O

推荐关闭协议：

```text
registry lock
  -> take_fd_for_close：slot 立即变 None，更新 open/free 索引
release registry lock
  -> 查询 metadata/inode，释放相应锁
  -> SharedIoHandle::close
       ├─ inner Arc 最后一份：立刻 close_once
       └─ 仍有 I/O lease/其它 slot：只 drop 此 Arc，最后引用时 Drop close
```

这保证新 syscall 立刻看到 EBADF，同时旧的稳定 handle snapshot 可以完成已经开始的 I/O。

`get_io/get_io_for_task` 只在 inner Arc 唯一时返回裸 `&mut dyn VfsIoHandle`，否则 Busy。通用 syscall 应使用 `io_handle_for_task/fd_slot_for_task` 克隆稳定 handle，释放 registry 锁后 `with_io`；不要为绕过 Busy 使用裸指针。

## 6. prepared read 两阶段协议

目标：用户目标页 fault 或只复制一部分时，不能永久吞掉 pipe/TTY/device 输入，也不能错误推进文件 offset/random 状态。

```text
fd lookup -> SharedIoHandle::prepare_read(max_len)
  -> concrete PreparedRead
  -> acquire()：等待并取得 reservation/暂存 bytes
  -> copy_to_user
  -> lease.finish(VfsCopyProgress { copied, complete })
       ├─ Bytes(copied)
       └─ Fault（0 copied 且 copy 未完成）
  -> lease 提前 Drop：取消 reservation，数据重新可读
```

具体实现：

- pipe/FIFO/socketpair：包装 `IpcPipeReadLease`；
- console TTY：`TtyReadReservation`；
- character driver：`CharacterReadReservation`，finish 时短持 device lock；
- PTY：`PtyReadReservation`，处理 canonical/VMIN/VTIME；
- urandom：预留 PRNG state，partial copy 只推进 copied bytes，写入 entropy 在 active read 期间累积 mix；
- `/dev/zero/null`：生成型/空 lease；
- framebuffer/evdev：feature-gated reservation。

任何新增可消费设备必须实现 prepared read；只实现 `read(&mut user_buffer)` 容易在 user-copy fault 时丢数据。

## 7. 管道、FIFO 与 socketpair

### 7.1 anonymous pipe

`pipe_handle_pair_with_flags(nonblocking, direct)` 创建一对共享 `PipeEndpoint` 和合成 inode。读写、poll、容量查询/设置、O_NONBLOCK/O_DIRECT 都下传 ipc-pipe。

VFS 只做错误映射：WouldBlock、BrokenPipe、Interrupted、NoMemory 等。PIPE_BUF 原子性、段队列、wait queue 和容量限制以 pipe 实现手册为准。

### 7.2 named FIFO

全局 `NAMED_PIPES` key 为 `(mount_id,inode)`，值为 `Weak<NamedPipe>`。同一 inode 的 open 共享管道对象；最后强引用 Drop 清 registry weak entry。

open 规则由底层 `NamedPipe::open_read/open_write` 实现：blocking reader/writer 配对，nonblocking writer 无 reader 时 BrokenPipe 映射 `NoDevice`。RDWR 同时打开两个 endpoint，并避免自阻塞 open。

### 7.3 Unix stream pair

每一端由两条交叉 pipe 组成：A.write→B.read，B.write→A.read。resource kind 为 Unix，accmode=RDWR。`shutdown(how)` 关闭共享 endpoint 的方向，因此 dup/fork 所有描述符都观察到 shutdown。

它只是 socketpair 数据通道，不是完整 AF_UNIX namespace/socket state；bind/listen/connect、credentials 和 ancillary data 在 syscall/network glue 中另行维护。

## 8. TTY、PTY 与字符设备

### 8.1 控制台字符设备

若 driver registry 有 Serial，stdin/out/err 使用 `CharDevHandle`；否则用最小 ConsoleIn/Out。唯一 console input worker 从硬件最多读一个字节，释放 device lock 后交给 `tty::feed_input`，再处理 echo/控制事件。禁止多个 worker 同时消费同一 serial 输入。

TTY output 先经 line discipline `transform_output` 再写 console。input prepared-read 根据 canonical、VMIN、VTIME 和 O_NONBLOCK 选择 wait；signal 打断映射 Interrupted。

非 TTY character device 若 driver `prepare_read` 暂无数据：nonblock 返回 WouldBlock，blocking 当前用 `task::yield_now()` 轮询；这是效率限制，新增 driver 应考虑事件/waitqueue 能力。

RTC 仅将 ioctl 下传 driver；read Unsupported/EOF 由当前兼容逻辑处理。设备 `stat` 与 `fstat` 都用 path hash 合成 inode，避免工具误判节点被替换。

### 8.2 PTY

- `/dev/ptmx`：分配新 master/slave pair；
- `/dev/pts/N`：打开已存在且未锁定的 slave；
- `/dev/tty`：按当前 process SID 找 controlling terminal；必要时回退系统 console；
- slave metadata：major 136、minor=N、mode 0620、gid=5；
- master：major 5、minor 2。

PTY read 支持 canonical、VMIN/VTIME、nonblock、EOF、hangup 和 signal interruption。poll 返回 POLLIN/POLLOUT/POLLHUP。`SharedIoHandle` 安装时缓存 terminal id，使 CLOEXEC/close 不必等待正在阻塞的 PTY read。

### 8.3 用户图形 feature

`user-graphics` 暴露 framebuffer 和 evdev，不能与内核 GUI 同时拥有硬件。evdev 每 client 队列容量 256，溢出通过 `SYN_DROPPED` 通知；Linux input event 固定 24 字节。worker 从 driver poll 事件并广播，read lease 失败必须归还未提交事件。

未启用 feature 时初始化返回 false，worker 只 sleep，special-device 查询不暴露图形路径。

## 9. cwd/root/exec 元数据

`PerTaskCwdRegistry` 实际同时拥有：

```text
cwd_tables    owner -> cwd
root_tables   owner -> chroot-like root
exe_paths     owner -> /proc/<pid>/exe 数据源
argv_vectors  owner -> cmdline 数据源
env_vectors   owner -> environ 数据源
auxv_vectors  owner -> auxv 原始字节
owners/ref_counts
```

`CLONE_FS` 调用 `share_cwd_from_parent`，cwd/root 及这些 exec 观察数据全部共享同一 owner；无 CLONE_FS 逐值 copy。PATH_MAX 当前为 256，copy 时父 cwd 超限会退回 `/`，而不是返回错误——修改路径上限或 fork 语义时要审查这一兼容行为。

root 与 cwd 都是字符串，真正 dirfd/root confinement 和 symlink escape 防护在 VFS 路径解析层；本 registry 不验证目录是否仍存在，也不持 inode lease。

## 10. 文件锁数据结构

全局 key：

```text
InodeKey { mount_id, inode }
  -> Arc<InodeLocks>
       data: Mutex<InodeLockData>
         posix: Vec<PosixLock { pid,type,start,len }>
         flock: { shared_holders: Vec<owner_id>, exclusive }
       wait: WaitQueue
```

只允许 `VfsNodeType::File` 上锁。key 必须包含 mount id，不能只用 inode。

POSIX range：`len=0` 表示直到 EOF/无限；end 用 saturating 运算。对同 pid 新锁先切掉重叠旧区域再插入；unlock 能把旧锁拆成左右两段。F_GETLK 忽略自己的锁，返回最早冲突交集；无冲突只把 `l_type=F_UNLCK`，保留其它字段。

### 10.1 owner 语义

- POSIX record lock owner 是 `ProcessId`，线程共享；
- flock owner 是 concrete handle 的 `flock_owner_id`，duplicate 保持该 id；
- 读锁之间兼容，写锁与任何其它 owner 锁冲突；
- 当前实现让 POSIX lock 与 flock 也互相冲突，比部分 Linux 文件系统的独立语义更强。

阻塞 SETLKW/flock 使用 `wait_current_while`，条件在 data mutex 下重查，避免丢唤醒；Interrupted 返回给 syscall 映射 EINTR。unlock/成功改变/cleanup 后 `wake_all`。

### 10.2 close/exit 释放

Linux POSIX record locks：进程关闭该 inode 的任意 fd 时释放该进程在 inode 上所有 record locks；当前 `close_slot` 符合此规则。进程最后线程退出还调用 `release_process_all_locks`，避免孤儿未 reap 时永久残留。

flock 应随 open-file description 最后引用释放。当前 `close_slot` 在每个 descriptor close 时都会调用 `release_flock_owner`，即使同 owner 仍有 dup/fork fd；这会过早解锁，是已知语义缺口。修复应把 flock cleanup 放到 OFD 的最后引用/`close_once` 生命周期，而不是每个 slot removal。

`drop_inode_if_empty` 先检查 data 为空再从全局表 remove；新增并发操作时要避免“检查为空→另一个线程取得旧 Arc 并加锁→全局 entry 被删除→同 inode 出现两套锁状态”的竞态。当前 get/remove 没有 generation 校验，压力测试必须覆盖这一窗口。

## 11. 新增 fd syscall 实例

例：新增 `fcntl` 类操作：

```text
syscall handler 复制参数
  -> current TaskId
  -> fd::with_registry
       -> fd_slot_for_task(fd)，克隆稳定 SharedIoHandle + flags/class
  -> 释放 registry 锁
  -> 若 FD_PATH_ONLY 且操作不允许：EBADF
  -> handle.with_io(|io| ...)
  -> VfsError 精确映射 errno
```

若操作会安装/替换 fd（dup、pidfd_getfd）：

```text
registry 锁：snapshot source
release
  -> fallible duplicate concrete handle
registry 锁：再次校验/安装目标，取出 displaced
release
  -> close displaced，反馈或记录错误
```

不要持 registry 锁做 user copy、FS writeback、pipe wait、driver ioctl 或调度。

例：新增一种设备 handle：

1. 定义 resource kind、accmode、metadata identity；
2. 明确 duplicate 后共享哪些状态；
3. 实现 prepared read reservation/rollback；
4. nonblock 返回 WouldBlock，blocking 使用 waitqueue 并支持 signal；
5. 状态改变后先更新状态再 wake，poll 与 read 条件一致；
6. close 幂等，最后引用才关闭底层对象；
7. 注册 special path、metadata 和 open 三个入口；
8. 测试 dup/fork/CLOEXEC/O_PATH/close-race/poll/EFAULT。

## 12. 常见故障定位

| 症状 | 优先检查 |
|---|---|
| fork 后 close 影响父 slot | table 是否 COW；是否误用 CLONE_FILES share |
| fork 后 offset 不共享 | concrete handle duplicate 是否共享 OFD state |
| dup 时偶发 Busy | live handle 被长 I/O 锁住且 snapshot 不可用 |
| close 卡死全系统 fd 操作 | 是否持 registry 锁进入 handle.close/writeback |
| fd 数越来越大不复用 | free_fds/open_counts 与 table 是否同步 |
| 低于 RLIMIT 仍 EMFILE | shared owner 的 open_count 与 task rlimit |
| exec 后 CLOEXEC 仍在 | take/close 两阶段或 shared table owner |
| close_range unshare 后 fd 消失 | duplicate 失败被 `.ok()` 静默降级 |
| pipe/TTY EFAULT 后数据丢失 | prepared read Drop/cancel/partial commit |
| blocking device 占满 CPU | driver read 没有 waitqueue，使用 yield polling |
| dup 一个 fd 后 close 另一个导致 flock 消失 | 当前每 slot 提前 release_flock_owner 缺口 |
| 文件锁永久阻塞 | process exit cleanup、inode key、wake_all |
| 同 inode 出现互不冲突的两套锁 | empty-entry remove 与并发 get 的竞态 |
| `/dev/tty` ENXIO | 当前 SID 没有 controlling terminal且无 console fallback |
| PTY read 永不返回 | canonical/VMIN/VTIME/nonblock/hangup 条件 |
| 设备 stat/fstat inode 不同 | special path hash 与 opened handle metadata |

调试 fd 泄漏至少记录：task、owner、refcount、table count、open_count、fd、slot flags、resource kind、inner Arc count、terminal id。只统计整数 fd 数无法区分共享表和共享 OFD。

## 13. 锁序与禁止事项

推荐顺序：

```text
短持 fd/cwd registry
  -> 克隆 SharedIoHandle/slot snapshot
release registry
  -> OFD mutex
  -> concrete handle 内部锁（page cache/pipe/TTY/device）
```

文件锁表：短持 `LOCK_TABLE` 只取得 `Arc<InodeLocks>`，随后释放，再持 per-inode data；等待时不持 data mutex，由 WaitQueue 原子重查条件。

禁止：

- registry 锁内等待 pipe/TTY/driver；
- registry 锁内 user copy 或分配不受限大 buffer；
- device lock 内投递 terminal signal或调用 scheduler；
- OFD mutex 与 fd registry 反向嵌套；
- close 后继续从 slot 裸引用访问 handle；
- 以 `Arc::strong_count` 作为跨线程长期不变量，只能用于紧邻的生命周期决定。

## 14. 修改检查清单

### fd table/clone

- 普通 fork、CLONE_FILES、spawn、clone rollback；
- owner 本人先 exit/unshare 时的 re-home；
- Arc table COW 后 indexes；
- RLIMIT、最低空洞、dup2/dup3 displaced close；
- CLOEXEC 与 exec 失败/成功边界；
- 所有 close 在 registry 锁外。

### concrete handle

- `duplicate` 是否共享正确的 offset/status/backend；
- `close` 幂等且最后引用语义正确；
- read/write/read_at/seek/poll/ioctl 能力；
- O_PATH/accmode 入口校验；
- prepared-read fault/partial/drop；
- metadata inode/mount id 和 file-lock key；
- O_NONBLOCK 状态在 dup/fork 间是否共享。

### file locks

- len=0、溢出和负 offset 由 syscall 规范化；
- 同 pid 合并/覆盖/拆分；
- read/write 冲突矩阵；
- F_GETLK 返回交集和 pid；
- blocking wait signal interrupt；
- close-any-fd 的 POSIX cleanup；
- last-OFD 的 flock cleanup；
- process exit 与 PID reuse。

## 15. 回归矩阵

静态门禁：

```bash
cd os
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

功能回归：

| 类别 | 必测场景 |
|---|---|
| fd alloc | close/reuse、minfd、RLIMIT、稀疏大 newfd |
| dup | offset/status共享、CLOEXEC 独立、dup2 replacement |
| fork | table COW、OFD 共享、父子不同 close 顺序 |
| clone | CLONE_FILES 实时共享、owner 先退出 |
| exec | CLOEXEC 普通/pipe/PTY/dirty file，失败回滚 |
| close_range | range、CLOEXEC、UNSHARE、duplicate failure |
| read lease | full/partial/EFAULT/Drop、两 reader 竞争 |
| pipe/FIFO | EOF/EPIPE/nonblock/open配对/poll/capacity/direct |
| socketpair | 双向、shutdown、dup 后方向关闭 |
| TTY/PTY | canonical/raw、VMIN/VTIME、signal、hangup、poll |
| device | serial/RTC/null/zero/random，stat=fstat |
| locks | GETLK/SETLK/SETLKW、split、flock、dup close、exit |
| cwd/root | CLONE_FS/copy、chdir/chroot、exec proc 数据 |
| concurrency | I/O 阻塞期间 dup/close/exec、fd registry 压力 |
| long run | forkheavy + pipe/PTY + open/close，heap/object计数回落 |

`self_test` 当前只聚合 pipe API smoke、character-driver API、TTY/character read lease 和 urandom read lease；不能证明 registry clone/exit、文件锁或真实阻塞并发。任何“通过”结论都要注明实际覆盖。
