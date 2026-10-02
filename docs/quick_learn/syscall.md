# WaterOS 系统调用：从 `ecall` 到子系统

这篇文档的目标不是背调用号，而是让你能在答辩中解释一条 syscall 的完整生命周期：用户态如何进入内核、内核如何找到实现、参数和用户指针怎样处理、状态由哪个子系统维护，以及错误和阻塞怎样返回。

## 先记住一句总括

WaterOS 对外提供 Linux generic 64 风格 ABI。架构相关代码只负责保存/恢复 trap frame；syscall API crate 定义调用号、参数槽位和 errno；`impl-kernel` 按语义实现 `sys_*`，再由一个稠密函数指针表按调用号分发。handler 本身是“ABI 适配层”，真正的长期状态仍属于 task、MM、VFS/FS、IPC、网络和平台定时器。

## 一次调用的时序

```text
用户 libc/汇编
  -> RISC-V/LoongArch syscall 指令（RISC-V 为 ecall）
  -> arch trap 入口保存寄存器到 TrapContext
  -> os/src/trap_handler.rs
       读取 nr 和 a0-a5，必要时切到内核地址空间
       -> dispatch_syscall_from_trap
  -> syscall_nr_dispatch.rs
       稠密表查找 ArgHandler / SpecialHandler
  -> sys::<domain>::sys_xxx
       解码、校验、copy_from/to_user、调用领域 API、转换 errno
  -> isize 返回值写回 a0
  -> sepc 前进一个 syscall 指令；必要时处理信号/EINTR 重启
  -> 恢复 TrapContext，sret/eret 回用户态
```

RISC-V 的 syscall 号在 `a7`（`x17`），六个参数在 `a0-a5`（`x10-x15`）；`TrapContext::syscall_args` 原样封装成 `SyscallArgs`，因此 API 层不依赖具体 ISA。LoongArch 走同一个上层契约，只在 arch trap frame 中解释不同寄存器。

### trap 层做什么

`os/src/trap_handler.rs::wateros_kernel_trap_handler` 先判断 trap 是否来自用户态，建立当前任务的权威 trap frame，并在需要时激活内核页表。遇到 `UserEnvCall` 后读取调用号和参数，调用 `dispatch_syscall_from_trap`。普通 syscall 返回后，trap 层推进用户 PC 并把结果写入返回寄存器；`execve` 成功会替换整个用户地址空间和 trap frame，所以不能再按普通路径推进 PC。`rt_sigreturn` 是特判：它从用户信号帧恢复寄存器，而不是进入普通分发表。

## ABI 契约与错误

`wateros-syscall-api/api-v0` 是实现无关的公共边界：

* `SyscallArgs` 是固定长度的 `usize` 槽位数组，顺序与 ABI 参数寄存器一致；它不替 syscall 检查参数。
* `UserRet` 约定成功为非负值，失败为负的 Linux errno。
* 内核内部使用 `KernelResult<T> = Result<T, ErrNo>`，`ErrNo` 保持正数，只有 `UserRet::from_error` 统一编码为 `-errno`。
* 未登记或越界调用号由 `sys_enosys` 返回 `-ENOSYS`，不会默认为成功。

handler 的安全边界通常是：先检查长度/flag/整数溢出，再通过 `user_copy` 访问用户内存；内核缓冲使用 `fallible_buf` 限制大小；完成领域操作后再把完整结果复制回用户。不能持有 VFS、IPC 或 socket 锁执行可能缺页、调度或用户拷贝的操作。

## 分发器如何工作

`syscall_nr_dispatch.rs` 把最大已登记调用号 `EPOLL_PWAIT2` 作为表上界，`ARG_SYSCALL_TABLE` 存放标准 `fn(SyscallArgs) -> UserRet` handler，`SPECIAL_SYSCALL_TABLE` 用于 `exit/getpid/brk/yield` 等返回签名不同的函数。查表是 O(1) 的静态数组索引，空槽和越界统一走 `ENOSYS`。宏只负责生成表，不隐藏业务语义。

`dispatch_syscall_from_trap` 还负责记录 syscall 次数和 `/proc/<pid>/io` 的读写统计。返回 `EINTR` 时，trap 层依据 `is_restartable_syscall` 对阻塞型读写、wait、socket、SysV 消息/信号量等调用重新进入；不可重启的调用把 `-EINTR` 原样交给用户。

## 按语义理解 syscall

| 领域 | 典型调用 | 作用 | 状态真正在哪里 |
| --- | --- | --- | --- |
| 文件/VFS | `openat`, `read`, `write`, `statx`, `renameat2`, `pipe2`, xattr | 路径解析、fd I/O、目录和文件属性 | VFS fd session、page cache、ext4/devfs/procfs |
| 任务/进程 | `clone`, `execve`, `waitpid`, `exit`, `sched_*`, `pidfd_*` | 创建、替换、等待和调度任务 | task 的 PCB/TCB、进程关系、调度器 |
| 内存 | `mmap`, `munmap`, `mprotect`, `brk`, `madvise`, `shm*` | 地址区间、权限、堆和共享内存 | MM 地址空间、页表、frame allocator、IPC SHM |
| IPC/信号 | `futex`, `rt_sigaction`, `kill`, `eventfd`, `signalfd`, SysV msg/sem | 同步、异步通知、队列和计数器 | ipc registry、signal state、wait queue、task |
| 网络 | `socket`, `bind`, `connect`, `accept4`, `sendmsg`, `recvmsg`, `setsockopt` | socket 生命周期和数据收发 | socket fd、AF_UNIX、smoltcp/network |
| 多路复用 | `poll`, `ppoll`, `epoll_*`, `pselect6` | 等待多个 fd 就绪 | `poll_engine`、epoll fd 和各对象 readiness |
| 时间 | `clock_gettime`, `nanosleep`, POSIX timer、`timerfd_*` | 时钟读取、睡眠和定时通知 | platform timebase、task/signal timer |
| 凭证 | `setuid`, `getgroups`, `capget/capset`, `prlimit64` | 身份、组、能力和资源上限 | cred registry、task 侧表 |
| 杂项 | `ioctl`, `mount`, `sync`, `sysinfo`, `getrandom`, `reboot` | 设备控制、挂载、同步和平台能力 | driver、VFS/FS、platform |

### 文件 I/O：以 `read` 为例

`sys/fs/io.rs::sys_read` 先取得当前任务的 fd I/O lease，检查 fd 类型和终端前台权限；零长度直接成功，空指针返回 `EFAULT`。随后向 VFS 请求可读 lease：数据暂不可用时，非阻塞 fd 返回 `EAGAIN`，阻塞 fd 释放对象锁并让任务让出/等待，再醒来重新检查。拿到 lease 后用 `copy_to_user_progress` 分段复制；复制失败按已复制字节提交或回滚，避免数据被消费却没有交给用户。`write/readv/pread*` 共享同样的用户指针、长度上限和部分成功原则。

### 路径和 fd：以 `openat` 为例

handler 解码 dirfd、用户路径、flags、mode，先复制并验证 NUL 结尾的路径，再通过 VFS 的 `path_at` 结合 cwd/dirfd 解析 mount namespace，最后由 FS backend 打开 inode 并在 fd session 中分配槽位。`FD_CLOEXEC` 属于 fd 槽位，`O_NONBLOCK/O_APPEND` 属于共享打开描述；这一区分决定 dup、fork 和 exec 的行为。失败要释放已经创建的句柄并返回准确的 `ENOENT/ENOTDIR/EACCES/EMFILE` 等 errno。

### 内存：以 `mmap` 为例

`sys/mem/mmap.rs::sys_mmap` 检查长度非零、未知 flag、匿名/文件映射组合、页对齐 offset、fd 权限和可写共享映射的打开模式，把 Linux `prot/flags` 翻译成 MM 的权限和 `MmapRequest`。真正的区间冲突、页表建立、惰性分配、COW 和文件回写由 MM 完成；syscall 层只做 ABI 转换和错误映射。`munmap/mprotect/brk` 同理，不能在 syscall 层复制页表状态。

### 进程生命周期：`clone -> execve -> exit/wait`

`clone` 根据 flag 决定共享地址空间、fd 表、信号和凭证，task 层创建新的线程/进程实体并设置返回值：父线程得到子 ID，子线程从用户上下文继续。`execve` 复制路径和 argv/envp，解析 ELF、建立新地址空间和用户栈，安装新的 trap frame；成功后旧映像不再返回。`exit/exit_group` 发布稳定的退出状态并清理线程资源，`waitpid/waitid` 在子进程可回收前阻塞，收到信号返回 `EINTR`；reap 才完成父子关系和剩余资源回收。

### futex、信号和阻塞协议

futex 的快路径在用户态完成原子竞争，内核只在值匹配时登记 waiter 并睡眠，唤醒时按 key 找到等待者。所有阻塞 syscall 都遵循“锁内检查条件 -> 登记 waiter/序号 -> 解锁 -> 睡眠 -> 醒来重新检查”的循环，覆盖超时、信号、对象删除和伪唤醒；不能只检查一次，也不能持锁调度。信号 syscall 修改 signal action/mask/pending 状态，真正投递发生在返回用户态安全点；因此 syscall 返回前还要处理 signal frame 和可能的 restart。

### socket 与 poll/epoll

`socket` 创建 socket 对象并包装成 fd；`bind/listen/connect/accept` 改变 socket 状态机；`send* / recv*` 负责 sockaddr、iovec、msghdr 的 ABI 复制和阻塞语义，底层由 AF_UNIX 或 smoltcp 提供传输。`poll/epoll` 不复制一套 I/O 状态，而是查询各 fd 的 readiness 并注册等待者；醒来后重新扫描，返回就绪项数量。关闭、dup、fork 和 exec 都必须正确维护最后一个引用及 `CLOEXEC`。

## 新增 syscall 的答辩式检查清单

1. 调用号是否已在 `syscall-api/api-v0/src/number.rs`，且两种架构共用正确 ABI？
2. handler 是否放进正确语义目录，并由该目录 `mod.rs` 重新导出？
3. 是否登记到 `syscall_nr_dispatch.rs`，标准签名和特殊签名是否匹配？
4. 用户指针、长度、对齐、flag、溢出、部分成功和 `EFAULT` 是否明确？
5. 长期状态由哪个子系统拥有，clone/exec/exit/close 时如何共享和释放？
6. 阻塞时如何避免 lost wakeup，信号中断后是否返回或重启 `EINTR`？
7. 是否有 self-test、用户态 smoke/QEMU workload，以及 `git diff --check`？

## 常见答辩问题与可直接回答的话术

**问：为什么不用一个巨大的 match？**
答：当前调用号是 asm-generic64 的稠密编号，静态函数指针表提供 O(1) 查找，空槽统一 `ENOSYS`；宏只生成表，具体语义仍按模块拆分。特殊签名放第二张表，避免热路径产生不必要的包装层。

**问：如何保证用户指针安全？**
答：绝不直接解引用用户地址。先做长度/溢出和地址检查，再用 `user_copy` 分段复制；输出先在内核构造完整值，复制失败按 ABI 决定整体失败或提交已复制部分，并且不持有会导致调度或缺页的锁。

**问：syscall 层和 VFS/MM 的边界是什么？**
答：syscall 层只解码 Linux ABI、做参数校验、调用领域 API、把领域错误转成 errno。fd/path/inode 属于 VFS/FS，页表和地址区间属于 MM，进程状态属于 task；这样 clone/exec 等跨域操作由 task 编排而不是在 syscall 中复制状态。

**问：阻塞 syscall 会不会死循环？**
答：等待采用可重入循环：锁内检查，不满足则登记 waiter 后解锁睡眠，醒来重新检查；同时检查 `O_NONBLOCK`、超时、信号和对象关闭。被信号打断返回 `EINTR`，只有登记在 restartable 表中的调用由 trap 层重启。

**问：execve 为什么特殊？**
答：成功的 exec 已经替换地址空间、用户栈和 trap frame，原 syscall 不应再把旧 `sepc` 前进或把旧返回值写回；失败才按普通 syscall 返回负 errno。这是防止成功后继续执行旧映像的关键。

**问：当前是不是“实现了所有 Linux syscall”？**
答：不是。项目覆盖运行 BusyBox、musl/glibc、LTP 所需的大量 generic64 syscall，但未登记或明确不支持的调用返回 `ENOSYS/EOPNOTSUPP`。答辩时应以分发表和对应 handler 为证据，不能把 ABI 兼容说成完整 Linux 内核兼容。

## 代码导航

* 入口与返回：`os/src/trap_handler.rs`
* 公共 ABI：`os/components/wateros-syscall/syscall-api/api-v0/src/{args,errno,number,return_value}.rs`
* 分发与 restart：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`
* 语义 facade：`.../src/sys/mod.rs`
* 用户拷贝与通用适配：`user_copy.rs`、`fallible_buf.rs`、`vfs_util.rs`、`mm_util.rs`
* 新增调用流程：[`adding-a-syscall.md`](../offline-development/adding-a-syscall.md)
