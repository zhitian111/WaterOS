# 内核功能补充实例手册

本文提供可以按步骤复用的修改样板。它不要求照抄具体名字，而是要求保留事务边界、生命周期和回归方式。新增普通 syscall 的编号与分发表流程见 [添加系统调用](adding-a-syscall.md)。

下面的片段用于说明当前工程采用的写法，并非脱离上下文即可复制的完整实现。使用前应点击源码链接
检查最新签名、错误类型、feature 和生命周期钩子；语法与所有权模式见
[WaterOS 常用 Rust 写法](rust-patterns.md)。

## 实例一：新增简单查询 syscall

现有 `getcpu` 位于 [`sched.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs)，适合作为“两个可空输出指针”的最小实例：

```text
读取当前 CPU（内核值）
  -> cpu_ptr != NULL 时 copy_to_user_struct(u32)
  -> node_ptr != NULL 时 copy_to_user_struct(u32)
  -> 成功返回 0
```

实现步骤：

1. 在 generic64 API 中确认 syscall number，不要凭印象复制其他架构号。
2. 在正确 domain 文件中实现 handler，参数只从 `SyscallArgs` 读取。
3. 用明确 ABI 宽度的类型，如 `u32`，不要直接写回 Rust `usize`。
4. 可空指针只在非零时访问；非可空输出为零应返回 `EFAULT`。
5. 从 domain `mod.rs` 导出 handler。
6. 在 dense dispatch 对应编号槽注册，并检查没有覆盖已有槽。
7. 写用户态测试：两个输出、分别为 NULL、指向跨页尾部、只读映射和无效地址。

查询两个输出时存在部分可见性：第一个写回成功、第二个 `EFAULT` 时，第一个值已经可见。这通常符合逐字段 copy 语义；若 ABI 要求原子可见，必须先验证全部范围或用单结构体一次写回。

### 对应代码

[`sys_getcpu`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs)
展示了可空输出和 ABI 宽度：

```rust
let cpu_ptr = args.arg(0);
let node_ptr = args.arg(1);
let cpu = platform::arch::cpu::current_cpu_id().raw() as u32;
let node = 0u32;

if cpu_ptr != 0 {
    if let Err(error) = copy_to_user_struct(cpu_ptr, &cpu) {
        return UserRet::from_error(error);
    }
}
if node_ptr != 0 {
    if let Err(error) = copy_to_user_struct(node_ptr, &node) {
        return UserRet::from_error(error);
    }
}
UserRet::from_success(0)
```

这里没有把内部 `CpuId` 或 `usize` 直接复制给用户，也没有对合法的 NULL 可选输出返回 `EFAULT`。

## 实例二：新增带输入/输出长度的 syscall

`getsockopt` 展示了长度指针协议，源码在 [`sockopt.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sockopt.rs)：

```text
校验 fd 是 socket
  -> 从 optlen_ptr 读取 u32 用户容量
  -> 限制最大内核缓冲大小
  -> 后端生成完整 value
  -> write_len = min(value.len, user_len)
  -> copy_to_user(optval, prefix)
  -> 把实际 write_len 写回 optlen_ptr
```

通用规则：

- 长度字段本身的 ABI 宽度必须准确；`socklen_t` 当前按 `u32`。
- 在分配前做上限检查，使用 `try_kbuf` 等可失败分配，禁止用户控制无限 Vec。
- 输入 buffer 必须确认实际 copied 长度等于请求长度。
- 用户输出复制失败时不要提交会消费后端状态的操作。
- 后端 `Unsupported` 要按 level/type 映射成 `ENOPROTOOPT` 或 `EOPNOTSUPP`，不能统一成功。

测试至少覆盖 0 长度、超大长度、NULL value、NULL length、短 buffer、未知 option、错误 fd 类型。

### 对应代码

[`sys_getsockopt`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sockopt.rs)
先读取用户容量，后端生成完整值，最后复制可容纳的前缀：

```rust
let user_len = match copy_from_user_struct::<u32>(optlen_ptr) {
    Ok(value) => value as usize,
    Err(error) => return UserRet::from_error(error),
};
if user_len > SYSCALL_SOCK_IO_MAX {
    return UserRet::from_error(ErrNo::EINVAL);
}

let value = match socket.get_sockopt(level, optname) {
    Ok(value) => value,
    Err(error) => return UserRet::from_error(getsockopt_error(error, level)),
};
let write_len = value.len().min(user_len);
if write_len > 0 {
    match copy_to_user(optval, &value[..write_len]) {
        Ok(copied) if copied == write_len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }
}
```

真实函数随后把 `write_len` 写回 `optlen_ptr` 并转换成 `UserRet`。新增类似 ABI 时应先确认规范要求
写回“完整所需长度”还是“实际复制长度”，不能从本例推断其他 syscall。

## 实例三：新增一个可读写 fd 类型

现有 [`eventfd.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs) 是完整模板。结构分三层：

```text
EventFdInner
  counter / nonblocking / reservation
        由 Mutex 保护

EventFdState
  Arc 共享对象 + WaitQueue

EventFdHandle
  Box<dyn VfsIoHandle> 放进 per-task fd table
```

### 必须实现的句柄语义

| 方法 | 目的 |
| --- | --- |
| `read` 或 `prepare_read` | 返回数据；有消费语义时优先 lease |
| `write` | 解析固定 ABI 字节并原子更新状态 |
| `metadata` | 提供稳定 node type/mode/inode |
| `duplicate` | dup/fork 时共享正确的底层对象 |
| `poll_revents` | 无副作用地报告 readiness |
| `poll_wait_for_ticks` | 在无锁状态睡眠并重新验证条件 |
| `open_status_flags` / setter | 支持 `fcntl(O_NONBLOCK)` 等 OFD flags |
| `open_accmode` | 让 transfer/fcntl 路径正确校验访问模式 |

### read 的 reserve-copy-commit

eventfd 不能在把 8 字节复制给用户前就扣减 counter：

```text
reserve_read
  -> 锁内检查 counter 和现有 reservation
  -> 记录唯一 reservation id/value
  -> 解锁
copy_to_user
  -> 成功：finish(commit=true) 扣 counter
  -> 失败：finish(false) 或 lease Drop 取消 reservation
  -> wake_all 让其他 reader 重试
```

任何“读取后状态改变”的 fd 都应采用此样板：pipe、socket、signalfd、inotify、timerfd。lease Drop 必须可取消且不 panic；否则 syscall 被 signal 中断或中途返回会永久占住 reservation。

对应的预留循环位于
[`EventFdState::reserve_read`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs)：

```rust
loop {
    {
        let mut inner = self.inner.lock();
        if inner.read_reservation.is_none() && inner.counter != 0 {
            let reservation = EventReadReservation {
                id: inner.next_read_id,
                value: if semaphore { 1 } else { inner.counter },
            };
            inner.read_reservation = Some(reservation);
            return Ok(reservation);
        }
        if inner.nonblocking {
            return Err(VfsError::WouldBlock);
        }
    }
    if self.wait.wait_current_while(|| {
        let inner = self.inner.lock();
        inner.read_reservation.is_some() || inner.counter == 0
    }) == task::TaskWaitResult::Interrupted {
        return Err(VfsError::Interrupted);
    }
}
```

花括号刻意缩短 `inner` guard 的生命周期；进入 waitqueue 前已经释放自旋锁。完整实现还会递增
reservation ID，并在 commit 时核对 ID 与 value。

### 创建 syscall 的回滚

```text
校验 flags
  -> 构造 handle
  -> fd::alloc_fd
  -> 若 CLOEXEC: set_fd_flags
       -> 失败则 close_fd 回滚
  -> 返回 fd
```

一旦 fd 返回用户，所有权已提交。返回前的任何失败都不能把匿名 fd 留在表内。

[`sys_eventfd2`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs)
中的 CLOEXEC 失败回滚是最小参考：

```rust
let event_fd = match fd::alloc_fd(Box::new(handle)) {
    Ok(fd) => fd,
    Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
};
if flags & EFD_CLOEXEC != 0 {
    if let Err(error) = fd::set_fd_flags(event_fd, FD_CLOEXEC) {
        let _ = fd::close_fd(event_fd);
        return UserRet::from_error(vfs_error_to_errno(error));
    }
}
UserRet::from_success(event_fd)
```

### 回归

- 初始值 0/非 0、普通与 semaphore 模式；
- 8 字节以外读写、`u64::MAX`、溢出；
- blocking/nonblocking、signal interrupt；
- poll 与真正 read/write readiness 一致；
- dup 后共享 counter，关闭一个 fd 不影响另一个；
- fork 继承与 CLOEXEC；
- 无效用户缓冲不消费 counter；
- 最后 fd 关闭时 waiter 被唤醒且对象释放。

## 实例四：给已有 fd 增加 ioctl/fcntl/option

先决定状态属于哪里：

- descriptor flag（例如 CLOEXEC）属于 fd slot，各 dup 出来的 fd 可不同；
- open-file status flag（例如 NONBLOCK）属于共享打开对象，dup/fork 应共同看到；
- 设备/协议选项属于底层 handle 或 socket；
- task 策略不应塞进 fd handle。

实现路径：

```text
syscall handler
  -> 校验 command/level/name 与 ABI 参数
  -> fd lookup + resource kind/downcast
  -> 小输入 copy_from_user 到固定结构
  -> handle 方法更新状态
  -> 必要输出 copy_to_user
  -> 映射 VFS/network/driver error
```

不要在 syscall 文件按 fd 数字另建一份“option map”，否则 dup、fork、close、fd 重用都会产生状态漂移。若确实需要 syscall 辅助索引（如 epoll/unix socket），必须在 copy/share/drop/rollback 全生命周期同步。

### 对应代码

[`fcntl_setfl`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fcntl.rs)
保留不可由 `F_SETFL` 修改的位，只更新共享打开对象中的可变子集：

```rust
vfs::fd::with_current_io(fd, |handle| {
    let preserved = handle.open_status_flags() & !F_SETFL_MASK;
    handle.set_open_status_flags(preserved | ((arg as u32) & F_SETFL_MASK))
}).map_err(vfs_error_to_errno)?;
```

相对地，`fcntl_setfd` 调用 `vfs::fd::set_fd_flags` 修改 fd slot 的 `FD_CLOEXEC`。阅读这两个函数可以
直接看到 descriptor flag 与 open-file status flag 的所有者差异。

## 实例五：新增 task-local 状态

timer slack 的实现位于 [`task.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs)，以 `TaskId -> TimerSlack` 注册表保存。

新增状态前先决定：

| 事件 | timer slack 当前语义 | 新状态需要明确 |
| --- | --- | --- |
| 首次读取 | 懒创建默认值 | 是否允许懒创建 |
| fork/clone | `copy_timer_slack`，child default/current 取 parent current | copy/share/reset |
| exec | 当前保留 | preserve/reset |
| exit | `drop_timer_slack` | exit 还是 reap 删除 |
| procfs | 回调 `timer_slack_for_task` | 是否公开与缺失默认值 |

实现清单：

1. registry key 必须用真实内部 `TaskId`。
2. 每次访问的锁临界区保持短小，不在锁内 user copy、日志或等待。
3. clone 创建回滚也要删除 child 状态。
4. exit cleanup 应幂等，因为 reap 路径可能再次兜底。
5. 若 zombie 查询需要它，则延迟到 reap；否则 exit 立即释放。
6. 添加压力测试确认表项数随任务数回落。

### 对应代码

timer slack 的复制和删除保持锁内操作短小：

```rust
static TIMER_SLACKS: Mutex<BTreeMap<usize, TimerSlack>> = Mutex::new(BTreeMap::new());

pub(crate) fn copy_timer_slack(parent: usize, child: usize) {
    let mut slacks = TIMER_SLACKS.lock();
    let current_ns = slacks.entry(parent)
                           .or_insert(TimerSlack {
                               default_ns: DEFAULT_TIMER_SLACK_NS,
                               current_ns: DEFAULT_TIMER_SLACK_NS,
                           })
                           .current_ns;
    slacks.insert(child, TimerSlack { default_ns: current_ns, current_ns });
}

pub(crate) fn drop_timer_slack(task_id: usize) {
    TIMER_SLACKS.lock().remove(&task_id);
}
```

源码见 [`task.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs)。
这段代码只展示表本身；新增状态还必须搜索 copy/drop 函数的全部调用点，确认 fork、clone、失败回滚
与退出路径都已接入。

## 实例六：新增 `/proc` 只读节点

procfs API 在 [`procfs-api`](../../os/components/wateros-fs/fs-procfs/procfs-api/api-v0/src/lib.rs)，实现由 `path.rs`、`view.rs`、`render.rs`、`callbacks.rs` 分工。

### 静态全局节点

例如新增 `/proc/example`：

1. 在 path/node 分类中加入节点枚举和路径解析。
2. `metadata` 标成普通只读文件，mode 与 size 语义一致。
3. 在 `read`/`read_range` 分发到 render 函数。
4. 根目录 `read_dir` 增加目录项。
5. 数据来自其他组件时，在 procfs API 定义窄的函数指针类型。
6. 在 `callbacks.rs` 保存 `Option<fn...>` 并提供 register/query。
7. 在状态所有者已初始化后、挂 procfs 前注册回调。

### `/proc/<pid>` 节点

还需处理 PID 到 leader TaskId 的解析、进程退出竞态和目录枚举。读取开始时目标存在、格式化前退出是正常竞态，应返回一致的 `NotFound` 或已经取得的稳定快照，不能解引用失效任务对象。

### 锁边界

```text
procfs read
  -> 复制已注册函数指针并释放 callback 锁
  -> callback 获取状态所有者锁并复制快照
  -> 释放状态锁
  -> render Vec/String
```

当前 callback 查询先复制函数指针再调用，避免持 callback registry 锁跨组件调用。新回调必须保持这一模式。

对应实现在
[`callbacks.rs`](../../os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/callbacks.rs)：

```rust
pub fn register_task_timer_slack_lookup(f: TaskTimerSlackLookup) {
    *TIMER_SLACK_LOOKUP.lock() = Some(f);
}

pub(crate) fn timer_slack_for(leader: TaskId) -> u64 {
    let lookup = *TIMER_SLACK_LOOKUP.lock();
    lookup.map(|f| f(leader)).unwrap_or(0)
}
```

路径节点还需分别接入
[`path.rs`](../../os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/path.rs) 的解析/目录项，
以及 [`view.rs`](../../os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/view.rs) 的 metadata/read
分发。只增加 callback 不会自动生成 `/proc` 文件。

### procfs 回归

- `stat/open/read/read_range/getdents/readlink` 与节点类型一致；
- offset 到 EOF、短 buffer、多次分段读取；
- 目标进程在 read 前/中/后退出；
- 未注册 callback 的退化行为；
- SMP 下并发读，不持跨组件嵌套锁；
- 内容单位、换行和列格式符合 Linux 工具预期。

## 实例七：新增 socket option

职责分两层：syscall 处理 ABI，network 处理 socket 状态。

1. 在 network backend 为 TCP/UDP 正确类型实现 `set_sockopt/get_sockopt`。
2. 决定 option 是仅保存兼容值，还是实际影响发送、接收、超时或 poll。
3. ABI handler 复用已有长度上限和 user-copy 流程。
4. 为错误 socket kind 返回 `ENOPROTOOPT`，未知 level/name 不返回假成功。
5. dup 后验证 option 是否共享；当前 `SocketShared` 表示 OFD 级共享。
6. fork 与最后关闭回归，确认没有 syscall 侧孤立 map。

仅为了让探测程序通过而保存 option 时，文档和代码注释必须明确它不会改变协议行为；会影响正确性的 option（timeout、reuse、buffer limit 等）不能静默空实现。

### 对应代码

network 后端中的两类 option 应明确区分。`TCP_NODELAY` 会真实改变协议行为：

```rust
let enabled = sockopt_bool(optval)?;
let socket = self.sockets.get_mut::<tcp::Socket>(handle);
socket.set_nagle_enabled(!enabled);
socket.set_ack_delay(if enabled { None } else { Some(Duration::from_millis(10)) });
self.socket_meta_mut(handle)?.tcp_nodelay = enabled;
return Ok(true);
```

而 `IP_TOS` 当前只校验后兼容接受，源码注释明确 smoltcp 尚无逐 socket TOS 接口。完整实现见
[`sockopt.rs`](../../os/components/wateros-network/network-impl/impl-smoltcp/src/stack/sockopt.rs)。增加 option 时应
把“保存值”“改变实际行为”“只作兼容探测”三种情况写清楚，并让 get 与 set 保持一致。

## 实例八：新增阻塞操作

标准循环：

```text
loop {
    锁内检查条件；满足则提交并返回
    nonblocking 则返回 EAGAIN
    解锁
    wait_current_while(|| 再次锁内检查条件)
    被 signal/exit interrupt 则返回 EINTR 或进入退出路径
}
```

必须考虑：检查到入队之间的 lost wakeup、虚假唤醒、超时换算、signal interrupt、`exit_group` 远程唤醒、对象最后 owner Drop、poll readiness 与实际操作条件一致。不能持 spin lock 调度或睡眠。

### 对应代码

[`signalfd`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signalfd.rs)
展示了“尝试消费—检查 nonblock—登记信号等待—睡眠—再检查”的实际控制流：

```rust
loop {
    let mask = self.state.mask();
    if let Some(first) = ipc::signal::take_pending_record(task_id, mask) {
        // 真实实现继续收集有限数量的记录，并构造可回滚 read lease。
        return build_lease(first);
    }
    if self.state.inner.lock().nonblocking {
        return Err(VfsError::WouldBlock);
    }
    ipc::signal::begin_signal_wait(task_id, mask).map_err(|_| VfsError::NoTask)?;
    let result = self.state.wait.wait_current_while(|| !self.state.pending_for(task_id));
    let _ = ipc::signal::end_signal_wait(task_id);
    if result == task::TaskWaitResult::Interrupted && !self.state.pending_for(task_id) {
        return Err(VfsError::Interrupted);
    }
}
```

片段中的 `build_lease` 是为突出控制流而写的缩写；真实代码还使用 `try_reserve_exact`，并在构造
缓冲失败时把已经取出的 pending signal 放回队列。

## 实例九：给 file-backed mmap 增加能力

需要同时审查 syscall、VFS、MM 和 FS：

```text
mmap 参数/flags/errno
  -> 从 fd handle 取得稳定 file identity / mapping lease
  -> MM 建 VMA（file offset、shared/private、prot）
  -> fault 时按页从 handle/page cache 填充
  -> private write: COW
  -> shared write: dirty tracking
  -> msync/munmap/exec/exit: writeback
  -> TLB shootdown + VMA/PTE/frame 释放
```

新增 flag 时不要只在 `sys_mmap` 接受位；必须说明它对 fault、fork、mprotect、msync、munmap 与销毁的行为。回归要用跨页、非页对齐文件尾、truncate 后访问、fork COW 与共享写回。

### 对应代码

[`sys_mmap`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs)
在进入 MM 前完成 ABI 与 fd 能力校验：

```rust
if len == 0 {
    return UserRet::from_error(ErrNo::EINVAL);
}
if offset % PAGE_SIZE != 0 {
    return UserRet::from_error(ErrNo::EINVAL);
}
if mf.contains(MapFlags::SHARED) && perm.writable()
    && accmode & O_ACCMODE != O_RDWR
{
    return UserRet::from_error(ErrNo::EACCES);
}
```

实际建立映射通过受控闭包取得地址空间，并由包装函数统一处理页表 flush：

```rust
mm::user_aspace::with_user_aspace_mut_and_flush_if_changed(handle, |aspace| {
    let mut alloc = GlobalPhysFrameAllocator;
    let base = MmapOps::mmap_file_lazy(aspace, &mut alloc, req, file_size, loader)?;
    Ok((base.0, mf.contains(MapFlags::FIXED)))
})
```

真实分支还区分 anonymous、device、只读 lazy file 和可写 shared file；此处不能把某一个分支当成
全部 mmap 语义。

## 每个实例提交前的统一审查

- ABI 类型、对齐、NULL、短 buffer、溢出和未知 flag 已测试。
- 状态放在正确所有者，没有以 syscall map 绕过底层模型。
- 创建以 publish/返回 fd 为 commit，commit 前失败完整回滚。
- 阻塞前释放 spin lock，唤醒后重新检查条件。
- copy-to-user 失败不会错误消费数据或推进 offset。
- dup/fork/clone/exec/exit/reap 行为均已定义。
- poll/epoll readiness 与真实操作完全一致。
- errno 保留错误层次，不返回假成功。
- 单核和 SMP 都跑过，资源计数在重复测试后回落。
