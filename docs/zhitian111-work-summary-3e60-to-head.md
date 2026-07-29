# zhitian111 工作讲解稿

## 使用说明

本稿整理范围为 `3e60fe4209256cf94239861526a0226b9432775d`（包含该提交）到当前 `HEAD`，只统计作者为 `zhitian111` 的提交。

当前范围的最后提交为：

当前范围共包含 24 个提交。

```text
37ee1907 [feat] implement per-thread signal alternate stacks
```

建议讲解顺序是：先讲多核基础设施，再讲文件系统和路径解析，之后讲用户程序执行链，最后讲观测能力与 syscall 补齐。这样能从“内核能否安全运行”自然过渡到“测试程序能否正确运行”。

## 一、先给出整体结论

这一阶段的工作重点不是单个 syscall，而是把 WaterOS 从“能启动用户程序”推进到“能在多核环境中稳定运行测试工作负载”：

1. 为地址空间建立 CPU 使用跟踪，并完成 TLB shootdown 的平台/固件接口。
2. 修复 `another_ext4` 的硬链接和历史镜像普通文件识别问题，同时处理 fd 表的 I/O 生命周期竞态。
3. 统一 VFS 路径和符号链接语义，使 `openat`、`fstatat`、`readlinkat`、exec 等路径行为一致。
4. 修复动态 ELF、shebang、解释器和 final workload 的启动链路。
5. 补齐 `eventfd2`、`readlinkat`、procfs 信息、线程信号备用栈等测试依赖。
6. 补齐 `fchdir`、`fadvise64` 等测试常用兼容 syscall。
7. 让多核日志、串口输出、在线 CPU 数量和系统 uptime 可观测且一致。

## 二、多核地址空间与 TLB

### 要解决的问题

一个用户地址空间可能同时被多个 CPU 使用。修改页表或销毁地址空间时，只刷新当前 CPU 的 TLB 会留下其他 CPU 的旧映射，造成错误访问甚至 use-after-free。原有接口也没有区分“调度 IPI”和“TLB shootdown IPI”。

### 主要提交

- `3e60fe4`：跟踪地址空间当前活跃 CPU。
- `0076530`：为 IPI 增加 `IpiKind` 类型。
- `28ed0dc`：优先使用 OpenSBI/固件提供的远程 TLB fence，并保留软件 IPI fallback。
- `5040189`：合并并整理调度器多核改动，涉及 scheduler CPU/lifecycle/tasks/wait 和 bringup。

### 涉及目录

```text
os/components/wateros-mm/mm-api/api-v0/
os/components/wateros-mm/mm-impl/impl-sv39/
os/components/wateros-mm/mm-impl/impl-loongarch64/
os/components/wateros-platform/platform-api/api-v0/
os/components/wateros-platform/platform-impl/
os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/
```

### 关键代码片段

地址空间生命周期 API 增加 CPU enter/leave hook：

```rust
pub type AspaceCpuHook = fn(usize, base::cpu::CpuId);

pub fn register_aspace_cpu_hooks(enter: AspaceCpuHook, leave: AspaceCpuHook) {
    *ASPACE_CPU_ENTER.lock() = Some(enter);
    *ASPACE_CPU_LEAVE.lock() = Some(leave);
}
```

SV39 实现用 bit mask 记录当前使用地址空间的 CPU：

```rust
active_cpus: AtomicU64,

pub fn mark_active(handle: usize, cpu: CpuId) {
    cell.active_cpus.fetch_or(1u64 << cpu.raw(), Ordering::AcqRel);
}
```

平台层区分 IPI 原因，并提供远程 TLB flush 接口：

```rust
pub enum IpiKind {
    Reschedule = 1 << 0,
    TlbShootdown = 1 << 1,
}

fn flush_tlb_remote(mask: CpuMask) -> PlatformSmpResult<()>;
```

TLB 更新时先尝试固件远程 fence，失败或不支持时再发送带类型的 software IPI：

```rust
match platform::smp::flush_tlb_remote(targets) {
    Ok(()) => return,
    Err(PlatformSmpError::Unsupported) => {}
    Err(error) => log::warn!("[tlb] remote flush failed: {:?}", error),
}
```

**讲解重点：** 这里的核心不是简单“增加一个 IPI”，而是建立了“谁正在使用地址空间、哪些 CPU 需要失效、如何等待完成”的闭环。

## 三、文件系统与 fd 生命周期

### 3.1 another_ext4 兼容

### 主要提交

- `fc03b5e`：在 `another_ext4` 适配层实现 hardlink。
- `7cf4c92`：兼容早期镜像中缺少 `S_IFREG` 的普通文件 inode，并调整日志/串口相关基础设施。

### 涉及目录

```text
os/components/wateros-fs/fs-impl/impl-another-ext4/
os/vendor/another_ext4/src/ext4_defs/
```

### 关键代码片段

适配层 hardlink 的流程是：查找源 inode、检查目标父目录和目标冲突，再调用底层 link：

```rust
let child = lookup(fs, existing_path)?;
let (parent_path, name) = parent_name(new_path)?;
let parent = lookup(fs, parent_path)?;

if lookup(fs, new_path).is_ok() {
    return Err(FsError::Exists);
}
fs.link(child, parent, name).map_err(map_error)?;
fs.flush_all();
```

初赛镜像中部分 `/etc` 占位文件的 mode 只有权限位，没有 `S_IFREG`。兼容逻辑将这类 legacy inode 识别为普通文件：

```rust
_ if self.bits() & InodeMode::PERM_MASK.bits() != 0
    => FileType::RegularFile,
```

这解决了 `/etc/passwd` 写入时的 `NotAFile`，也使 `/bin/ls` 等硬链接能够共享同一个 inode。

### 3.2 fd 表竞态

### 主要提交

- `fbcfcc4`：解决 fd 表在并发 I/O、close、任务退出中的竞态。
- 后续工作树中继续将该机制收敛为统一 I/O lease，并覆盖标准 fd。

### 涉及目录

```text
os/components/wateros-vfs/src/fd.rs
os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs
```

原问题是 `with_current_io` 临时把句柄从 fd 表取出，I/O 期间任务退出或 fd 表清理后，恢复句柄会得到 `BadFd`。现在使用 lease/inflight 和槽位锁：

```rust
let (owner, handle_ptr, slot_lock) =
    with_fd_registry(|reg| reg.begin_io_for_task(task_id, fd))?;
let _slot_guard = slot_lock.lock();
let result = f(unsafe { &mut *handle_ptr });
drop(_slot_guard);
with_fd_registry(|reg| {
    reg.end_io_for_owner(owner, fd);
    Ok(())
})?;
```

任务退出时若仍有 active I/O，不立即销毁 fd 表，而是延迟清理；旧表结束后再回收，新任务复用相同 task id 时分配新的 owner。

**讲解重点：** 标准输入 fd 不是内核全局常量，而是每个任务 fd 表中的槽位。因此 fd 0 同样需要生命周期保护，不能因为它是 stdin 就绕过并发管理。

## 四、VFS 路径、符号链接和 exec

### 4.1 统一路径解析

### 主要提交

- `98b7d01`：增加完整 VFS 符号链接解析。
- `dda8390`：统一 syscall 路径解析。

### 涉及目录

```text
os/components/wateros-vfs/vfs-api/api-v0/src/resolve.rs
os/components/wateros-vfs/src/lib.rs
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/
```

路径解析现在统一处理 cwd、绝对/相对路径、`.`、`..`、中间符号链接和最终符号链接：

```rust
let follow = !is_final || final_symlink == FinalSymlink::Follow;
if follow {
    if let Some(target) = read_link(candidate.as_str())? {
        if followed == MAX_SYMLINKS {
            return Err(VfsError::TooManySymlinks);
        }
        // 将 target 与未处理的剩余路径重新组合后继续解析
    }
}
```

随后 `openat`、`fstatat`、`faccessat`、`statfs`、`truncate`、xattr、acct 等 syscall 都使用统一解析结果，避免每个 syscall 各自处理 cwd 和符号链接。

### 4.2 动态 ELF、解释器和 shebang

### 主要提交

- `457d58e`：通过 VFS 读取动态可执行文件。
- `5bf05fb`：保持 ELF interpreter 语义，不在 exec 层错误重写。
- `48b6a8d`：final workload 通过 shebang 正确执行。
- `f1035aa`：修正决赛脚本路径。

### 涉及目录

```text
os/components/wateros-mm/mm-api/api-v0/
os/components/wateros-mm/mm-impl/impl-sv39/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/
os/src/user_bringup_busybox.rs
os/src/user_bringup_common.rs
```

执行链现在是：

```text
用户 execve
  -> 统一路径解析
  -> VFS 读取 ELF/script
  -> 解析 interpreter 或 shebang
  -> 通过 VFS 加载动态解释器
  -> 建立用户地址空间并启动程序
```

兼容 BusyBox shell 的路径映射仍集中在 exec 入口，而不是散落在 ELF loader：

```rust
if matches!(abs_path, "/bin/sh" | "/usr/bin/sh" | "/bin/bash") {
    return String::from("/glibc/busybox");
}
```

**讲解重点：** 这些提交解决的是同一条“用户程序启动链”上的不同层次问题：路径找到、符号链接正确、脚本解释器正确、动态 ELF 通过 VFS 装载。

## 五、procfs、CPU 和系统时间观测

### 主要提交

- `85ad35e`：增加单调递增 proc uptime。
- `1f021d8`：统一系统 uptime 口径。
- `3d94f34`：向用户态暴露 online CPU 数量。

### 涉及目录

```text
os/components/wateros-fs/fs-procfs/
os/components/wateros-task/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/
```

系统启动时记录统一的 uptime 基准，procfs 和 `sysinfo` 使用相同来源，避免启动早期和运行期时间不一致：

```rust
pub fn monotonic_uptime_ticks() -> u64 {
    current_ticks().saturating_sub(BOOT_TICKS)
}
```

`/proc/cpuinfo`、`/proc/stat` 和调度相关查询使用在线 CPU 状态，而不是仅使用配置的最大 CPU 数：

```rust
let online = task::online_cpu_mask();
let ncpu = online.count_ones();
```

## 六、串口、日志和构建说明

### 主要提交

- `5a4cbee`：在 UART 层串行化 console 输出。
- `7cf4c92`：日志默认附带 CPU id。
- `39b8ce8`：记录 Buildstorm kernel runtime 任务和测试说明。

### 涉及目录

```text
os/components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/
os/components/wateros-runtime/runtime-console/
os/components/wateros-runtime/runtime-logging/
docs/todo/buildstorm-kernel-runtime-tasks.md
```

多核下直接并发写 UART 会造成日志交错，因此输出在 UART/console 层集中加锁；日志格式增加 CPU 信息后，可以判断异常发生在哪个 CPU：

```text
[WaterOS][cpu=5] [INFO] [vfs] self_test mkdir ok
```

这部分不是纯粹的显示优化，而是多核故障定位的基础设施。

## 七、IPC、进程接口和信号

### 7.1 eventfd2 与 clone3

`4c91abf` 增加 `eventfd2` syscall，从 syscall API、分发器到 kernel 实现完整接通；`9eb0479` 接受 clone3 中暂未使用的 pidfd 字段，避免用户程序因传入该参数被错误拒绝。

涉及目录：

```text
os/components/wateros-syscall/syscall-api/api-v0/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/
```

eventfd 的核心是 64 位计数值和阻塞/非阻塞读写语义：

```rust
pub(crate) fn sys_eventfd2(args: SyscallArgs) -> UserRet {
    let initial = args.arg(0) as u32 as u64;
    let flags = args.arg(1);
    let handle = EventFdHandle::new(initial, nonblock, semaphore);
    let event_fd = fd::alloc_fd(Box::new(handle))?;
    UserRet::from_success(event_fd)
}
```

### 7.2 readlinkat、`/proc/<pid>/exe` 和备用信号栈

`d0e32c5` 补齐 `readlinkat` 和 proc executable links；`37ee190` 为每线程实现 signal alternate stack 相关 API 和 syscall 分发。

涉及目录：

```text
os/components/wateros-fs/fs-procfs/
os/components/wateros-mm/mm-api/api-v0/
os/components/wateros-ipc/ipc-signal/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/
```

备用信号栈的目标是让信号处理在用户栈损坏或栈空间不足时仍有独立栈可用：

```rust
pub struct AlternateSignalStack {
    pub sp: usize,
    pub size: usize,
    pub active_frames: usize,
}

pub(crate) fn sys_sigaltstack(args: SyscallArgs) -> UserRet {
    // 读取用户态 stack 描述，校验后按线程保存并返回旧值
}
```

### 7.3 文件描述符工作目录和访问建议

`eb0601d` 增加 `fchdir`，使程序可以从目录 fd 设置 cwd；`6ce46a7` 增加 `fadvise64` 的兼容语义，对访问模式进行参数校验并返回 Linux 兼容结果。

涉及目录：

```text
os/components/wateros-syscall/syscall-api/api-v0/
os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/
```

`fchdir` 的实现通过 fd 获取目录句柄，再把目录路径写入当前任务 cwd：

```rust
let path = fd::with_current_io(fd, |handle| {
    handle.directory_path()
         .map(String::from)
         .ok_or(VfsError::NotDirectory)
})?;
cwd::set_task_cwd(task_id, path.as_str())?;
```

`fadvise64` 作为兼容接口存在，不改变文件内容，只校验 fd、offset、length 和 advice：

```rust
match advice {
    POSIX_FADV_NORMAL | POSIX_FADV_RANDOM | POSIX_FADV_SEQUENTIAL |
    POSIX_FADV_WILLNEED | POSIX_FADV_DONTNEED | POSIX_FADV_NOREUSE => Ok(0),
    _ => Err(ErrNo::EINVAL),
}
```

## 八、提交清单（按时间正序）

以下是范围内全部作者为 `zhitian111` 的提交；`5040189` 是 merge commit，内容主要是把调度器和 bringup 的并行改动合并到当前主线。

| Commit | 主题 |
|---|---|
| `3e60fe4` | 跟踪活跃 CPU，支持地址空间 TLB shootdown |
| `fc03b5e` | another_ext4 hardlink |
| `7cf4c92` | 修复 another_ext4 `NotAFile`、串口竞态和 CPU 日志 |
| `5040189` | 合并 scheduler 多核与 bringup 改动 |
| `0076530` | IPI 类型区分 |
| `f1035aa` | 修正决赛脚本路径 |
| `fbcfcc4` | fd 表竞态修复 |
| `98b7d01` | 完整 VFS 符号链接解析 |
| `dda8390` | 统一 syscall 路径解析 |
| `457d58e` | 动态 ELF 通过 VFS 加载 |
| `5bf05fb` | 保留 executable interpreter 语义 |
| `48b6a8d` | final workload shebang 执行 |
| `85ad35e` | monotonic proc uptime |
| `1f021d8` | 统一 system uptime |
| `3d94f34` | 暴露 online CPU |
| `5a4cbee` | UART console 输出串行化 |
| `39b8ce8` | Buildstorm runtime 任务文档 |
| `d0e32c5` | readlinkat 和 proc executable links |
| `4c91abf` | eventfd2 descriptors |
| `9eb0479` | 接受 clone3 未使用 pidfd 字段 |
| `28ed0dc` | 使用固件 remote TLB fence |
| `37ee190` | per-thread signal alternate stacks |
| `eb0601d` | fchdir for directory descriptors |
| `6ce46a7` | fadvise64 compatibility semantics |

## 九、收尾时可以这样讲

“这段时间我主要做的是把多核运行所需要的基础闭环补齐。底层先解决地址空间在哪些 CPU 上活跃，以及页表更新如何通知其他 CPU；中间层统一 fd、VFS 路径和符号链接语义，修掉 another_ext4 和任务退出时的竞态；上层再把动态 ELF、shebang、procfs、eventfd 和信号备用栈这些测试依赖接通。最后通过 UART 串行化、CPU id、online CPU 和统一 uptime，让多核问题能够被观察和定位。结果是 final/pre workload 不再依赖各模块自己的路径或 fd 特判，而是走统一的内核接口。”
