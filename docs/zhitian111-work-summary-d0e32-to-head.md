# zhitian111 工作成果说明（`d0e32c52` 至 `b255b18e`）

> 统计范围：`d0e32c520e65568`（含）至当前 `HEAD=b255b18e93cfe797`，只分析
> `os/` 目录，作者过滤为 `zhitian111 <2367651943@qq.com>`。
>
> 统计时间：2026-07-29。本文描述的是已经提交的成果；工作区中尚未提交的
> `sys/fs/io.rs` 和 `user_bringup_busybox.rs` 改动不计入统计。

## 一、适合汇报时先讲的结论

这两天的工作不是单点补 syscall，而是围绕“让 WaterOS 能稳定承载 Linux
兼容性测试和并行编译负载”完成了一轮横跨内核各层的兼容性与并发正确性收口。
可以概括成四句话：

1. **补齐 Linux 用户态所依赖的内核接口。** 新增或完善 `readlinkat`、
   `eventfd2`、`sigaltstack`、POSIX timer、`fchdir`、`fadvise64`、
   `rseq`、`riscv_hwprobe`、`utimensat`、凭据和信号权限等语义。
2. **把单核或低并发下“看似可用”的路径改到 SMP 下正确。** 重点修复 fd
   继承锁、页缓存/ext4 并发、进程退出与回收、调度重调度、远程 TLB、控制台输出。
3. **打通真实工作负载。** 处理 BusyBox/脚本解释器、Unix jobserver
   socketpair、TCP listen/accept、分段 loopback 发送，并增加 buildstorm 并行编译探针。
4. **从临时兼容迈向可维护实现。** 大部分接口已接入正式 VFS、task、signal、
   scheduler 抽象；仍有少数兼容层是保守实现或 sidecar 状态，需要后续下沉。

共 **55 个提交、97 个不同文件**。其中 54 个非合并提交按逐提交 `numstat` 统计为
**4,258 行新增、973 行删除、5,231 行总变动**。这里的“行数”表示开发活动量，
不等同于当前代码净增加；同一文件被多次修改会累计。提交 `f316ec27` 是信号分支
集成提交，本文将它表述为“合并与适配成果”，不把合入分支的全部原始代码宣称为
个人独立实现，也不把其 combined diff 重复加到上述总数。

| 组件/工作域 | 新增 | 删除 | 总变动 | 汇报关键词 |
|---|---:|---:|---:|---|
| VFS、页缓存与 ext4 | 1,421 | 636 | 2,057 | fd 锁隔离、并发页缓存、ext4 持久化 |
| 系统调用与 IPC 兼容 | 905 | 82 | 987 | eventfd、文件接口、futex、ABI 兼容 |
| 信号与 POSIX 定时器 | 925 | 61 | 986 | altstack、timer、kill 权限、sigsuspend |
| 凭据与权限 | 329 | 24 | 353 | set-id、groups、用户指针错误传播 |
| 内存、平台与控制台 | 220 | 72 | 292 | lazy fault、remote TLB、32 CPU、串口 |
| 任务生命周期与调度 | 187 | 48 | 235 | 非 leader fork、退出发布、延迟回收 |
| 网络与 Unix 套接字 | 168 | 28 | 196 | backlog、loopback、jobserver |
| 构建、启动与测试 | 103 | 22 | 125 | final 配置、启动时序、buildstorm 探针 |

## 二、按组件说明工作成果

### 1. VFS、文件描述符、页缓存与 ext4

#### 做了什么

- 重构 fd session 中的打开文件状态：fork 后 fd 表仍共享“打开文件描述”
  所需的 offset/flags，但不再错误共享一次具体 I/O 操作的临时锁，解决父子进程
  并发读写或关闭时互相卡死。
- `execve` 清理 `CLOEXEC` socket 时增加同步，避免与其他线程同时访问 fd 表。
- 实现 `readlinkat`，补齐 `/proc/self/exe`、`/proc/<pid>/exe` 可执行文件链接，
  并让 fstatat 默认跟随符号链接。
- 实现目录 fd 驱动的 `fchdir`，完善 shell 脚本工作目录推导。
- 修复 ext4 chmod 时覆盖 inode 类型位的问题。
- 对页缓存和 another_ext4 做并发加固：明确锁顺序、缩短底层文件系统持锁区间、
  增加 inode/block cache、修复 extent 与目录更新；之后又分别修复淘汰失败隔离、
  同父目录 rename 链接保持、目录增长落盘。

#### 为什么做

并行编译会同时触发 fork/exec、共享 fd、目录增长、大量小文件读写、rename 和缓存
淘汰。原实现中的“全局或跨 fork 共享操作锁”以及 ext4 元数据更新不完整，会表现为
随机死锁、文件消失、目录项重启后丢失或一次回写失败拖垮整个缓存。

#### 涉及路径与行数

| 重点文件 | 新增 | 删除 | 作用 |
|---|---:|---:|---|
| `components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs` | 496 | 397 | fd session 状态和操作锁重构 |
| `components/wateros-vfs/src/fd.rs` | 89 | 54 | fd API、继承和 CLOEXEC 同步 |
| `components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs` | 261 | 63 | 并发页缓存、回写与淘汰隔离 |
| `components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs` | 29 | 6 | 页缓存与底层 FS 桥接 |
| `components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs` | 56 | 19 | ext4 入口锁粒度和属性修复 |
| `vendor/another_ext4/src/ext4_defs/cache.rs` | 175 | 0 | ext4 元数据缓存 |
| `vendor/another_ext4/src/ext4_defs/extent.rs` | 55 | 47 | extent 更新正确性 |
| `vendor/another_ext4/src/ext4/extent.rs` | 82 | 5 | extent 分配、增长与落盘 |
| `vendor/another_ext4/src/ext4/low_level.rs` | 46 | 18 | rename、链接和低层元数据 |
| `vendor/another_ext4/src/ext4/dir.rs` | 24 | 4 | 目录增长与持久化 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs` | 31 | 3 | `readlinkat` |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/cwd.rs` | 36 | 0 | `fchdir` |

本组件合计新增 1,421 行、删除 636 行。上表省略少量接线文件，完整范围可由文末命令复核。

#### 代表性代码

`components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs:410`

```rust
let resolved = resolve_path_at(dirfd, path.as_str())
    .and_then(|path| resolve_symlinks(path.as_str(), FinalSymlink::NoFollow))?;
let target = vfs::read_symlink_absolute(resolved.as_str())?;
let count = core::cmp::min(buf_size, target.len());
copy_to_user(buf_ptr, &target[..count])
```

这段代码体现了 `readlinkat` 的关键语义：相对路径按 `dirfd` 解析，但最后一级链接
不能被提前跟随；返回内容不自动补 `NUL`，并按用户缓冲区长度截断。

页缓存的核心约束写在
`components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:5`：

```rust
// 1. files
// 2. per-file FileEntryInner
// 3. state（持锁期间不得调用下层块设备 I/O）
// 4. 根卷 SharedRwFs
```

这不是普通注释，而是此次并发修复的设计边界：统一拿锁顺序，禁止持有缓存全局状态锁
时下探块设备，降低锁反转和长临界区风险。

#### 当前现状

- fd 继承、CLOEXEC、符号链接、目录切换等路径已经接入正式 VFS。
- 页缓存已经具备 per-file 读写锁、脏页回写和单次淘汰失败隔离。
- ext4 的目录增长、rename 和 extent 更新有针对性修复。
- 风险在于 `os/vendor/another_ext4/` 属于第三方代码，本轮为解决阻塞问题做了较大修改；
  后续升级上游版本时需要单独维护补丁。

#### 后续建议

1. 把 ext4 改动整理成独立补丁集，并补充断电/重新挂载后的持久化回归。
2. 对页缓存增加并发 truncate、rename、unlink、writeback 错误注入测试。
3. 用锁依赖检查或 debug instrumentation 持续验证文档中的锁顺序。

---

### 2. 通用系统调用与 IPC 兼容层

#### 做了什么

- 完整增加 `eventfd2` 描述符，支持普通/信号量模式、阻塞/非阻塞读写、
  `poll` 就绪判断和 `CLOEXEC`。
- 增加 `fadvise64` 保守兼容语义、`rseq` 显式降级、保守版
  `riscv_hwprobe`，使 libc 或应用能识别“内核知道该接口，但不夸大硬件能力”。
- 支持 console TTY 的 termios 查询/设置，避免常见 shell 和工具因 ioctl 失败退出。
- 私有 futex key 加入地址空间身份，避免不同进程相同虚拟地址错误命中同一 futex。
- 接受当前尚不实际返回 pidfd 的 `clone3.pidfd` 字段，改善新 libc 的兼容性。
- 完善 `utimensat` 的 flags、`UTIME_NOW/OMIT`、权限和用户内存检查。

#### 为什么做

这些接口常常不是业务程序直接调用，而是 glibc/musl、shell、GNU make、语言运行时
自动探测。简单返回 `ENOSYS` 会触发不合适的用户态分支；返回成功但语义错误则更危险。
因此本轮采用“能正确实现的完整实现，暂时做不到的保守声明”。

#### 涉及路径与行数

| 重点文件 | 新增 | 删除 | 作用 |
|---|---:|---:|---|
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs` | 228 | 0 | eventfd 状态机与 VFS handle |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/attr.rs` | 220 | 12 | `utimensat` 与属性权限 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/riscv_hwprobe.rs` | 104 | 0 | RISC-V 能力查询 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs` | 44 | 12 | console termios |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fadvise.rs` | 47 | 0 | fadvise64 |
| `components/wateros-ipc/ipc-futex/futex-api/api-v0/src/key.rs` | 44 | 5 | 私有 futex 地址空间隔离 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/futex.rs` | 13 | 19 | futex key 接线 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/rseq.rs` | 17 | 0 | rseq 降级接口 |

组件合计新增 905 行、删除 82 行。

#### 代表性代码

`components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/eventfd.rs:24`

```rust
struct EventFdState {
    inner: Mutex<EventFdInner>,
    wait: task::wait_queue::WaitQueue,
}

if inner.counter != 0 {
    let value = if self.semaphore { 1 } else { inner.counter };
    inner.counter -= value;
    self.state.wait.wake_all();
    return Ok(8);
}
```

这里将 counter 与等待队列绑定为一个可共享的打开文件状态；dup/fork 后仍看到同一计数器，
而 semaphore 模式每次只消费 1，符合 jobserver 等真实用法。

`utimensat` 当前在
`components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/attr.rs:274`：

```rust
if atime.is_none() && mtime.is_none() {
    return UserRet::from_success(0);
}
stat_times::set(&meta, atime, mtime);
```

#### 当前现状

- eventfd、termios、futex key 属于可实际参与并发工作的实现。
- `fadvise64`、`rseq`、`riscv_hwprobe` 刻意采用保守语义，重点是兼容且不误报能力。
- `utimensat` 暂时把时间存入 inode-keyed syscall sidecar，`stat/statx` 可观察，
  但它还不是文件系统元数据的真正持久化更新。

#### 后续建议

1. 在 VFS metadata API 增加可写时间戳，并将 `utimensat` 下沉至 ext4。
2. 随 RISC-V ISA 探测能力完善，逐项扩展 hwprobe，保持“只报告已验证能力”。
3. 增加 eventfd 溢出边界、signal semaphore、公平唤醒和 close-while-waiting 测试。

---

### 3. 信号、进程组与 POSIX 定时器

#### 做了什么

- 实现每线程 `sigaltstack`，维护备用栈配置和活跃信号帧计数。
- 实现 `timer_create/settime/gettime/getoverrun/delete`，并与 signal 投递连接。
- 重写 `kill` 目标解析：正确处理 `pid > 0`、`pid == 0`、`pid == -1`、
  `pid < -1`，支持进程组和广播。
- 加入真实/effective/saved UID 及 session 的信号权限检查，校验 tkill/tgkill
  线程目标。
- fork 时清除 parent-death signal。
- 修复 `sigsuspend` mask 过早恢复；参与集成新的 signal delivery safe-point、
  pselect/ppoll mask、停止/继续/终止语义。

#### 为什么做

LTP 信号测试和多线程运行时依赖非常细的 Linux 语义。目标选择错误可能把信号发给错误
进程；权限缺失属于安全问题；在发送 CPU 上直接终止远端正在运行的任务会造成资源仍在
使用却已被释放。备用栈和定时器则是 libc、语言运行时和 profiling 的基础能力。

#### 涉及路径与行数

| 重点文件 | 新增 | 删除 | 作用 |
|---|---:|---:|---|
| `components/wateros-ipc/ipc-signal/src/lib.rs` | 344 | 3 | altstack、POSIX timer 初始实现 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs` | 203 | 56 | signal syscall、目标和权限 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/posix_timer.rs` | 233 | 0 | POSIX timer syscall |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/kill_target.rs` | 131 | 2 | kill 参数分类和检查 |
| `components/wateros-ipc/ipc-signal/signal-impl/impl-core/src/registry.rs` | 4 | 0 | sigsuspend mask 修复 |

组件合计新增 925 行、删除 61 行；另外集成提交 `f316ec27` 对 signal/task/scheduler
共做了 462 行新增、181 行删除的 first-parent 合并适配，这部分不重复计入上表个人
逐提交统计口径。

#### 代表性代码

`components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs:818`

```rust
match classify_kill_target(pid) {
    KillTargetSelector::Process(pid) => ...,
    KillTargetSelector::CurrentProcessGroup => ...,
    KillTargetSelector::Broadcast => ...,
    KillTargetSelector::ProcessGroup(pgid) => ...,
}
```

`components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs:514`

```rust
if stack.size < MINSIGSTKSZ {
    return Err(ErrNo::ENOMEM);
}
if stack.sp == 0 || stack.sp.checked_add(stack.size).is_none() {
    return Err(ErrNo::EINVAL);
}
```

这两段分别体现“先把 Linux 参数空间分类，再查目标”和“用户栈必须做长度与地址溢出
检查”，避免将复杂语义散落在发送路径里。

#### 当前现状

- kill/tkill/tgkill 的目标与权限检查已成体系。
- altstack 和 POSIX timer 已有完整 syscall 表面。
- signal 核心随后经其他分支重构到 `signal-impl/impl-core`；`f316ec27` 已完成接口
  对接，因此早期 `ipc-signal/src/lib.rs` 中的部分实现已演化，不应只看初始提交评价现状。

#### 后续建议

1. 以多 CPU 压力测试覆盖 signal 到达、任务迁移、stop/continue/exit 的交叉时序。
2. 补 POSIX timer overrun、删除竞态、不同 clock id 和 fork/exec 生命周期测试。
3. 将 signal 集成提交拆分出可复核的回归用例，降低以后修改 safe-point 的风险。

---

### 4. 凭据与权限

#### 做了什么

- 完善 `setuid/setgid/setreuid/setregid/setresuid/setresgid/setfsuid/setfsgid`
  的 root 与非 root 权限规则，以及 real/effective/saved ID 的变更。
- 实现 supplementary groups 的读取/设置和用户缓冲区长度校验。
- 修复 `getres*id` 拷贝到用户空间失败时仍返回成功的问题。
- 为信号权限检查提供按进程查询凭据的接口。

#### 为什么做

凭据 syscall 的难点不是“写一个整数”，而是 Linux 对三套 ID 的迁移约束。
如果只提供表面接口，会让权限测试误通过，甚至允许普通用户提升权限；用户指针错误
不传播则会破坏 syscall 的可诊断性。

#### 涉及路径与行数

| 文件 | 新增 | 删除 |
|---|---:|---:|
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/mod.rs` | 122 | 24 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/setid.rs` | 142 | 0 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/cred/groups.rs` | 50 | 0 |
| `components/wateros-cred/cred-impl/impl-root/src/lib.rs` | 9 | 0 |
| `components/wateros-cred/src/lib.rs` | 6 | 0 |

组件合计新增 329 行、删除 24 行。

#### 当前现状与后续

当前已从“接口占位”提升到按 Linux 权限规则校验。下一步应补齐 capability、
user namespace（若项目范围需要）以及 exec setuid/setgid 文件位的端到端行为；
同时用非 root 身份跑完整 credential LTP 子集。

---

### 5. 任务生命周期与调度

#### 做了什么

- 支持非线程组 leader 发起 fork，修正新进程应继承的调用线程上下文。
- `exit_group` 主动释放 sibling 资源，但进程实体延迟到所有线程真正退出后再回收。
- 规范化用户传入的 exit code，再编码 wait status。
- 调整子进程退出顺序：先发布可观察的退出状态，再通知 waiters，避免父进程醒来后
  仍看不到 zombie/状态。
- 修复 scheduler 初始化在大 CPU 数下的栈溢出。
- timer tick 和 fallback 路径都尊重本地 pending reschedule，避免唤醒后不调度。

#### 为什么做

SMP 下“发出退出请求”不代表远端线程已经停止执行。过早释放进程资源会形成 use-after-free；
先 wake 再发布状态会形成丢唤醒式竞态。非 leader fork 是 pthread 程序常见路径，也是
并行编译和运行时的基础。

#### 涉及路径与行数

| 重点文件 | 新增 | 删除 |
|---|---:|---:|
| `components/wateros-task/task-impl/impl-core/src/process.rs` | 86 | 10 |
| `components/wateros-task/src/lifecycle.rs` | 26 | 7 |
| `components/wateros-task/src/lib.rs` | 11 | 10 |
| `components/wateros-task/src/spawn.rs` | 17 | 3 |
| `components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/lib.rs` | 18 | 11 |
| `components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs` | 8 | 2 |

组件合计新增 187 行、删除 48 行。

#### 当前现状与后续

关键状态发布顺序和资源生命周期已经显式化。后续应增加 fork/exec/exit/wait 与线程迁移
混合压力测试，并给 process/thread 状态机写一份唯一的合法转换表；调度侧建议记录
reschedule 原因和延迟，便于区分“没被唤醒”和“唤醒后没抢占”。

---

### 6. 网络驱动与 Unix socket

#### 做了什么

- Unix `socketpair` 接受 `SOCK_SEQPACKET`，并补全 jobserver 所需的 socketpair I/O、
  readiness 与 handle 接线。
- cagent TCP listen 使用调用者给定 backlog，不再固定或忽略。
- accept 状态转换前预留容量，避免已完成连接因内部队列扩容时机错误而丢失。
- loopback 分段发送在 syscall 返回前 flush，避免只发送首段或数据滞留。

#### 为什么做

GNU make/cargo 并行构建会使用 jobserver fd；编译器进程之间又会快速建立、接受大量
本地连接。接口存在但 readiness、backlog 或分段 flush 不正确，表现为随机卡住，
而不是稳定报错，定位成本很高。

#### 涉及路径与行数

| 文件 | 新增 | 删除 |
|---|---:|---:|
| `components/wateros-driver/driver-network/src/lib.rs` | 93 | 14 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs` | 47 | 4 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/socketpair.rs` | 16 | 10 |
| `components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/sendto.rs` | 12 | 0 |

组件合计新增 168 行、删除 28 行。

#### 当前现状与后续

针对本地并行构建的主要阻塞点已经处理。下一步应以高并发短连接、backlog 边界、
半关闭、对端提前 close、超大分段发送做压力测试，并将 cagent 特有 flush 行为逐步
收敛到统一 socket 发送完成语义。

---

### 7. 内存管理、SMP 平台与控制台

#### 做了什么

- 内核复制用户内存时可触发 lazy page fault，RISC-V 与 LoongArch 两端保持一致。
- RISC-V 地址空间切换使用 OpenSBI remote fence 做跨核 TLB 刷新，并补平台 API。
- 将支持的逻辑 CPU 数扩到 32，同时调整两架构启动汇编的 hart/cpu 过滤。
- final panic 关机前 flush UART。
- console 写入在争用时串行化，避免多核日志字符交叉和 console 重入。
- final 构建关闭 dashboard，降低干扰和额外开销。

#### 为什么做

用户页采用按需映射后，`copy_from_user/copy_to_user` 不能把“页尚未建立”直接当坏地址；
SMP 页表变化若只刷本地 TLB，会让其他 CPU 继续使用旧映射。控制台与 panic flush
则直接关系到并发故障是否留下可读证据。

#### 涉及路径与行数

| 重点文件 | 新增 | 删除 |
|---|---:|---:|
| `components/wateros-platform/src/lib.rs` | 38 | 15 |
| `components/wateros-mm/mm-impl/impl-sv39/src/user_access.rs` | 21 | 16 |
| `components/wateros-mm/mm-impl/impl-loongarch64/src/user_access.rs` | 21 | 16 |
| `components/wateros-platform/platform-api/api-v0/src/smp.rs` | 7 | 0 |
| `components/wateros-platform/platform-impl/impl-qemu-riscv64-opensbi/src/console.rs` | 10 | 2 |
| `components/wateros-platform/platform-impl/impl-qemu-loongarch64-virt/src/console.rs` | 5 | 1 |
| `components/wateros-mm/mm-impl/impl-sv39/src/kernel_elf.rs` | 50 | 3 |

组件合计新增 220 行、删除 72 行。

#### 当前现状与后续

两架构用户访问路径已同步修复，RISC-V remote fence 已接固件。32 CPU 支持目前更多是
容量和启动边界修复，仍应实际跑满 32 vCPU，验证 per-CPU 数组、hart 映射和调度器。
console 串行化保证正确性，后续可引入 per-CPU 缓冲降低全局锁争用。

---

### 8. 启动、运行时适配与测试工具

#### 做了什么

- 启动任务延迟到 runtime setup 完成后发布，避免任务先运行、依赖尚未初始化。
- bundled RISC-V ELF interpreter 路径重映射，脚本根据所在 glibc/musl 测试树选择
  对应 BusyBox/解释器；final SMP 测试固定使用 bundled BusyBox。
- 避免启动阶段破坏性裁剪 LTP 文件。
- 增加 guest buildstorm 并行编译探针，帮助复现 jobserver、文件系统和网络并发问题。
- final 内核构建关闭 dashboard。

#### 为什么做

这些改动把“内核单项测试通过”连接到“真实用户态负载能启动并持续运行”。尤其是
解释器路径和初始化发布顺序，失败时常表现为文件不存在或偶发早期崩溃，容易被误判为
文件系统或调度器故障。

#### 涉及路径与行数

| 文件 | 新增 | 删除 |
|---|---:|---:|
| `scripts/guest_buildstorm_parallel_probe.sh` | 61 | 0 |
| `src/user_bringup_common.rs` | 19 | 13 |
| `src/user_bringup_busybox.rs` | 7 | 6 |
| `src/user_bringup_bus.rs` | 4 | 1 |
| `Makefile` | 12 | 1 |
| `Cargo.toml` | 0 | 1 |

组件合计新增 103 行、删除 22 行。

#### 当前现状与后续

已经具备面向 buildstorm 的 guest 探针和更稳定的 final 启动路径。下一步应把探针纳入
固定回归流程，记录并发度、完成时间、失败阶段和 QEMU 配置；测试启动代码中的临时
兼容映射应逐步数据化，避免继续堆叠硬编码。

## 三、提交与成果索引

下面按讲解主题列出 55 个提交，便于演示时从结论跳回证据。

- **VFS/文件系统：** `d0e32c52` readlinkat/proc exe，`8d710ac9` fstatat 符号链接，
  `eb0601d0` fchdir，`6ce46a75` fadvise，`13836f7e` termios，
  `b629e2ff` ext4 chmod，`d11a6f26` fd 操作锁，`fc0d84cf` script cwd，
  `fba81834` ext4/page-cache 并发，`6e7e04dd` utimensat，`3d1e33c0` CLOEXEC，
  `7bffe932` rename link，`e7e0e41d` 目录增长，`f7f77481` 淘汰隔离。
- **IPC/信号/时间：** `4c91abf9` eventfd2，`37ee1907` sigaltstack，
  `d032621f` POSIX timer，`837d6b79` private futex，`3d326146` pdeath signal，
  `fa5e2c73` kill 进程组，`f501b89f` signal 权限，`c131ceb4` thread target，
  `526b2e0e` sigsuspend，`f316ec27` signal delivery 集成。
- **任务/调度：** `9eb04793` clone3 pidfd 兼容，`dde1ff5a` exit group，
  `dbd5b673` non-leader fork，`72cbf633` runtime 后发布任务，
  `70953cea` exit status，`f918b97c` scheduler 初始化，
  `bddd4979` child exit 发布，`a79c8d8c`、`d43500a6` reschedule，
  `f90fb208` 延迟进程回收。
- **凭据：** `75227c86` set-id 权限，`45907f09` groups 缓冲区，
  `4c25bd71` getresid 错误传播。
- **内存/平台：** `28ed0dca` remote TLB，`fe2ed496` lazy user page，
  `4d903674` 32 CPU，`5146c20a` panic UART，`0405b6c3` console 串行化。
- **网络/真实负载：** `a31840d1` seqpacket，`dba747a2` listen backlog，
  `1fec182a` segmented loopback，`0d25a32b` accept capacity，
  `f4d5bfb1` jobserver socketpair，`5cf76de4` buildstorm probe。
- **启动和兼容探测：** `6956541c` rseq，`5312a4ab` hwprobe，
  `597b482c` final dashboard，`8cfdc6b6` bringup 问题记录，
  `327f02af` ELF interpreter，`620f3cbb` bundled BusyBox，
  `76672612` 禁止破坏性 LTP 裁剪。

## 四、汇报时可以使用的讲述顺序

建议不要按 commit 时间逐条念，而按一次问题闭环来讲：

1. **目标：** 从“能启动测试”推进到“能跑 Linux 兼容测试和并行 buildstorm”。
2. **先补接口：** eventfd、signal stack、timer、文件和凭据 syscall。
3. **再解决并发：** fd 锁、futex key、页缓存/ext4、退出回收、调度唤醒。
4. **然后打通负载：** BusyBox/解释器、jobserver、TCP backlog、loopback。
5. **最后说明质量保障：** buildstorm 探针、panic 串口 flush、淘汰失败隔离。
6. **主动讲清技术债：** `utimensat` sidecar、保守 hwprobe/rseq、vendor ext4
   补丁维护，以及 32 vCPU 和高并发回归仍需持续验证。

一句话版本可以这样说：

> 这一阶段完成了 WaterOS 面向 Linux 用户态和并行编译负载的一轮系统性收口：
> 一方面补齐关键 syscall 和权限语义，另一方面重点修复 SMP 下 fd、文件系统、
> 信号、进程退出、调度和网络的竞态，并建立 buildstorm 探针；当前核心路径已打通，
> 后续重点是持久化语义、极端并发回归和第三方 ext4 补丁治理。

## 五、统计口径与复核命令

```bash
# 提交数（含起点提交，只看 os）
git log --format='%H' --author='zhitian111' \
  d0e32c520e65568^..HEAD -- os | wc -l

# 每次提交的文件级变动；同一文件多次修改会重复累计
git log --author='zhitian111' --format= --numstat \
  d0e32c520e65568^..HEAD -- os

# 提交清单
git log --format='%h%x09%ad%x09%s' --date=short \
  --author='zhitian111' d0e32c520e65568^..HEAD -- os
```

注意：

- 使用 `d0e32c...^..HEAD` 是为了包含起点提交本身。
- 行数来自 Git 文本 diff；二进制文件不计文本行数。
- 当前 HEAD 中还包含其他作者的提交，不能用一个简单的
  `git diff d0e32c^..HEAD` 作为个人工作量。
- “为什么做”和“后续建议”由提交内容、代码注释及上下游调用关系归纳；其中后续建议
  是工程判断，不代表已经承诺的排期。
