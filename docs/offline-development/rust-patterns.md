# WaterOS 常用 Rust 写法

本文不是完整的 Rust 教程，只整理阅读和修改 WaterOS 时最常遇到的语法、所有权模式与内核约束。
示例均来自当前工程的实际写法；代码变化时以链接指向的源码为准。

## 1. `no_std` 环境中的导入

内核组件不能默认使用 `std`。基础类型来自 `core`，需要堆分配的容器来自 `alloc`：

```rust
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;
use spin::Mutex;
```

常见对应关系：

| 普通 Rust 程序 | WaterOS 内核中 |
| --- | --- |
| `std::vec::Vec` | `alloc::vec::Vec` |
| `std::sync::Arc` | `alloc::sync::Arc` |
| `std::collections::*` | `alloc::collections::*` |
| `std::sync::Mutex` | `spin::Mutex` 或组件提供的锁 |
| `println!` | `log::*`、klog 或早期 console |

`spin::Mutex` 的 guard 仍遵循 RAII，但它不会让当前线程睡眠。临界区内不能等待调度、访问可能缺页的
用户内存或执行不受控的设备 I/O。

## 2. `Option`、`Result` 与系统调用返回值

组件内部通常使用 `Option<T>` 和 `Result<T, E>`；只有 syscall ABI 边界才转换成 `UserRet`。

### `let ... else`：缺少对象时立即返回

```rust
let Some(process) = task::current_process_snapshot() else {
    return UserRet::from_error(ErrNo::ESRCH);
};
```

它适合“后续逻辑必须使用该值”的分支，比嵌套 `if let` 更清楚。若错误需要区分，改用 `match`。

### `match`：保留错误层次

[`sys_getcpu`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs)
中的用户复制采用显式匹配：

```rust
if cpu_ptr != 0 {
    if let Err(error) = copy_to_user_struct(cpu_ptr, &cpu) {
        return UserRet::from_error(error);
    }
}
```

组件函数中可使用 `?` 提前传播错误：

```rust
let active = inner.read_reservation.ok_or(VfsError::Io)?;
inner.counter = inner.counter
                     .checked_sub(reservation.value)
                     .ok_or(VfsError::Io)?;
```

syscall handler 通常不能直接 `?`，因为其返回类型是 `UserRet` 而不是 `Result`。常见做法是把复杂逻辑
放进返回 `Result<usize, ErrNo>` 的辅助函数，handler 最后统一编码：

```rust
match operation() {
    Ok(value) => UserRet::from_success(value),
    Err(error) => UserRet::from_error(error),
}
```

`ErrNo` 在 Rust 内部保持正数；不要手工取负后再传给 `from_error`。

## 3. 所有权：`Box`、`Arc`、借用和复制

### `Box<dyn Trait>`：把不同句柄放入同一 fd 表

VFS fd 表保存实现 `VfsIoHandle` 的不同对象。创建 eventfd 时把具体类型擦除为 trait object：

```rust
let handle = EventFdHandle::new(initial, nonblocking, semaphore);
let event_fd = fd::alloc_fd(Box::new(handle))?;
```

trait object 适用于运行时需要在 file、pipe、socket、eventfd 等实现之间分派的边界。跨组件稳定能力应
优先放在 trait/API crate 中，不要在 syscall 层按 fd 数字维护平行类型表。

### `Arc<T>`：共享同一个打开对象

`Arc` 表示共享所有权，不等于线程安全。内部可变状态仍需要锁或原子变量：

```rust
struct EventFdState {
    inner: Mutex<EventFdInner>,
    wait: task::wait_queue::WaitQueue,
}

struct EventFdHandle {
    state: Arc<EventFdState>,
    semaphore: bool,
}
```

dup/fork 后需要共享 counter、offset 或 socket 状态时，复制 `Arc`；需要每 fd 独立的 `CLOEXEC` 时，
复制 fd slot 的 flag，而不是塞进共享对象。

### `Copy` 只用于小型值快照

ID、flag、固定 ABI 小结构常派生 `Clone, Copy`。持有 `Vec`、`Arc`、锁或资源句柄的类型通常不能
`Copy`，应显式 `clone` 或转移所有权，使引用计数和析构时机可见。

## 4. 锁作用域与显式 `drop`

锁 guard 离开作用域时自动释放。需要在唤醒、调度或跨组件调用前提前解锁时，使用嵌套作用域或
`drop(guard)`：

```rust
let mut inner = self.inner.lock();
inner.read_reservation = None;
drop(inner);
self.wait.wake_all();
```

推荐的阻塞结构是：

```rust
loop {
    {
        let mut state = object.lock();
        if condition(&state) {
            return commit(&mut state);
        }
        if state.nonblocking {
            return Err(VfsError::WouldBlock);
        }
    } // 在这里释放锁

    if wait.wait_current_while(|| should_keep_waiting())
        == task::TaskWaitResult::Interrupted
    {
        return Err(VfsError::Interrupted);
    }
}
```

闭包会在入队和唤醒边界再次检查条件，用于避免 lost wakeup；因此闭包必须短小，不能产生永久副作用。

## 5. RAII 与事务式提交

Rust 的 `Drop` 在内核里常用于失败回滚，但不能假定所有退出路径都会自然析构，例如从其他 CPU
强行把仍在内核栈上的任务标记为 Exited 就会跳过 guard。

eventfd 的读 lease 是典型 reserve/copy/commit：

```rust
impl Drop for EventFdReadLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            self.state.cancel_read(reservation);
        }
    }
}
```

正常复制后 `finish` 取走 reservation 并提交；中途 `EFAULT`、signal 或提前返回时，`Drop` 取消预留。
相关完整实现见
[`eventfd.rs`](../../os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs)。

创建资源时同样要明确 commit 点：fd 返回用户之前发生错误，应关闭已分配 fd；任务进入 Ready 之前
失败，应撤销所有 side table 和地址空间。

## 6. ABI 结构：`repr(C)`、宽度和字节序

用户态可见结构必须显式使用 C 布局和确定宽度：

```rust
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct UserTimespec {
    sec: isize,
    nsec: isize,
}

const _: () = assert!(core::mem::size_of::<UserSigInfo>() == 128);
```

注意：

- `usize/isize` 只在 ABI 明确等于目标指针宽度时使用；`socklen_t`、UID、flag 等通常是 `u32/i32`。
- 固定结构使用 `copy_from_user_struct`/`copy_to_user_struct`，不能把用户地址强转成 Rust 引用。
- 数值字节流按 ABI 使用 `to_ne_bytes/from_ne_bytes`；网络报文字段才通常使用大端。
- `repr(C)` 只保证布局规则，不会自动验证保留字段、对齐指针或语义范围。

## 7. 数值转换、溢出与可失败分配

用户控制的长度、数量和地址运算不能依赖 release 模式的整数回绕：

```rust
let byte_len = count
    .checked_mul(size_of::<UserEntry>())
    .ok_or(ErrNo::EINVAL)?;
let end = user_addr.checked_add(byte_len).ok_or(ErrNo::EFAULT)?;
let size = usize::try_from(meta.size).map_err(|_| ErrNo::EINVAL)?;
```

内核使用固定堆，不能直接对用户长度执行 `vec![0; len]`。先限制 ABI 上限，再使用 `try_reserve`、
`try_reserve_exact` 或项目的 `fallible_buf`。分配失败返回 `ENOMEM`，不要触发 allocator panic。

## 8. 静态注册表与函数指针回调

跨组件不能反向依赖时，procfs 使用窄函数指针注册回调：

```rust
static TIMER_SLACK_LOOKUP: Mutex<Option<TaskTimerSlackLookup>> = Mutex::new(None);

pub fn register_task_timer_slack_lookup(f: TaskTimerSlackLookup) {
    *TIMER_SLACK_LOOKUP.lock() = Some(f);
}

pub(crate) fn timer_slack_for(task: TaskId) -> u64 {
    let lookup = *TIMER_SLACK_LOOKUP.lock();
    lookup.map(|f| f(task)).unwrap_or(0)
}
```

关键点是先复制函数指针并释放 registry 锁，再调用回调。回调应返回稳定快照或拥有的数据，不能泄露
受锁保护对象的引用。完整代码见
[`callbacks.rs`](../../os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/callbacks.rs)。

## 9. 闭包与 `with_*` API

WaterOS 经常用闭包限制锁内对象或地址空间借用的生命周期：

```rust
let accmode = vfs::fd::with_current_io(fd, |handle| {
    Ok(handle.open_accmode())
})?;
```

```rust
let base = mm::user_aspace::with_user_aspace_mut_and_flush(handle, |aspace| {
    let mut alloc = GlobalPhysFrameAllocator;
    MmapOps::mmap(aspace, &mut alloc, request, None).map(|base| base.0)
})?;
```

闭包结束后借用不会逃逸，包装函数还可统一处理 fd 锁、页表 flush 和错误转换。不要为了“方便”返回
内部裸引用或把锁 guard 存到全局结构中。

## 10. trait、泛型与 `?Sized`

API crate 用 trait 描述跨实现能力：

```rust
fn remove_links(session: &mut (impl RootRwSession + ?Sized)) -> FsResult<()> {
    // 同时接受具体类型和 dyn RootRwSession
}
```

- `impl Trait` 表示调用点可传入任意满足约束的具体类型。
- `T: Trait` 适合需要多次引用类型参数或附加约束时使用。
- `dyn Trait` 表示运行时动态分派，通常放在 `Box`/`Arc`/引用后面。
- `?Sized` 取消泛型默认的 `Sized` 约束，使函数也接受 trait object。

trait 方法若要用于 `dyn Trait`，必须满足 object safety。新增泛型方法或返回 `Self` 可能使整个 trait
无法再构造成 trait object，修改 VFS/driver API 时尤其要检查。

## 11. `unsafe` 的使用边界

内核不可避免地需要汇编、MMIO、页表和 ABI 字节转换，但 `unsafe` 应集中在可审计的小函数内：

```rust
// SAFETY: destination points into the fixed payload array; size was checked
// against payload.len(), and source is a fully initialized repr(C) value.
unsafe {
    core::ptr::copy_nonoverlapping(src, dst, len);
}
```

每个 `unsafe` 块至少说明：

1. 指针为何有效、对齐且覆盖足够长度；
2. 生命周期和别名规则为何成立；
3. 哪个锁、引用计数或中断状态保证并发安全；
4. 调用者需要维持什么前置条件。

用户地址不能因“已经检查非零”就裸解引用；必须走 `user_copy`，让 MM 正确处理跨页、权限、COW 和
`EFAULT`。

## 12. feature、模块可见性与条件编译

本项目通过 Cargo feature 选择架构、profile 和组件实现。常见写法：

```rust
#[cfg(feature = "self_test")]
mod tests;

pub(crate) use wait::{sys_waitid, sys_waitpid};
```

- `pub` 是跨 crate API；`pub(crate)` 只在当前 crate 可见，应优先使用最小可见性。
- `mod foo` 声明模块，`pub(crate) use foo::bar` 才把 handler 暴露给领域聚合层。
- `cfg` 关闭的代码不会参与类型检查，因此至少执行 RISC-V、LoongArch 和相关 profile 的构建检查。
- 通用代码中不要到处散落架构 `cfg`；优先把差异封装进 platform/MM/driver 的 API 与实现 crate。

## 13. 日志、断言和测试代码

- 可恢复的用户错误返回 errno，不用 `assert!` 或 `panic!`。
- `debug_assert!` 只适合内部不变量，release profile 可能移除。
- 高频 syscall、tick、fault 路径避免格式化大对象；日志参数也可能延长 guard 或引用的生命周期。
- `#[cfg(test)]` 单测适合纯函数和数据结构；涉及用户地址空间、调度与文件系统时使用 QEMU guest 回归。
- 修复完成至少执行 `cargo fmt --check`、`git diff --check` 和两架构 `make check`，再运行对应 LTP/应用测试。

## 14. 阅读报错时的快速判断

| 报错 | 常见原因 | 优先处理 |
| --- | --- | --- |
| cannot borrow as mutable | 当前只有共享引用，或仍存在其他借用 | 缩短借用作用域；确认是否真需要 `&mut` |
| value used after move | 所有权已传给容器/函数 | 在提交前使用；共享所有权才 `Arc::clone` |
| trait is not dyn compatible | trait 出现泛型方法、`Self` 返回等 | 拆分 object-safe 能力或改静态分派 |
| future/closure may outlive borrowed value | 闭包保存了短生命周期引用 | 复制小快照或转移 `Arc`，不要保存 guard 引用 |
| cannot infer type | `collect`、`sum`、`map_err` 目标不明确 | 给局部变量、turbofish 或闭包返回值标类型 |
| invalid register on host target | 用宿主目标检查了架构汇编 | 使用 `make check ARCH=rv/la` |
| Send/Sync 不满足 | 对象含裸指针或非线程安全内部状态 | 不要盲目 `unsafe impl`；先定义所有权和锁协议 |

语法能通过编译只说明类型关系成立。涉及锁、等待、用户复制和生命周期时，还必须结合
[跨组件数据结构与生命周期](data-structure-lifetimes.md)与
[功能补充实例](feature-cookbook.md)检查运行时语义。
