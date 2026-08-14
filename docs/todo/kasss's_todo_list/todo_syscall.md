# WaterOS syscall 审计与完善清单

本文以 RISC-V64、LoongArch64 共用的 Linux asm-generic ABI 为准，记录
`wateros-syscall` 的真实实现状态。最近一次审计时间为 2026-08-14；当前分发表
包含 200 余个入口，但“已经分发”不等于“语义已经完整”。

## 1. 本轮已完成

| syscall | asm-generic nr | 当前语义 |
|---|---:|---|
| `sethostname` | 161 | root 可修改全局 nodename，`uname` 随后返回新值；最大 64 字节 |
| `setdomainname` | 162 | root 可修改全局 NIS domainname，`uname` 随后返回新值 |
| `readahead` | 213 | 完整校验 fd、访问模式、偏移和普通文件类型；当前作为页缓存性能提示处理 |
| `syncfs` | 267 | 校验 fd，随后同步当前唯一可写根卷及全局文件页缓存，并返回写回错误 |
| `reboot` | 142 | 校验 euid、Linux magic 和 command；支持 restart、halt、poweroff |
| `personality` | 92 | 支持查询及原生 `PER_LINUX`；不支持的执行域/标志返回 `EINVAL` |
| `splice` | 76 | 支持 file→pipe、pipe→file、pipe→pipe；pipe 输入使用 read lease，短写不丢数据；支持显式文件 offset 与 `SPLICE_F_NONBLOCK` |
| `tee` | 77 | 用 read lease 向第二条 pipe 复制数据并以 0 消费提交，输入 pipe 内容保持不变 |
| `vmsplice` | 75 | 校验 iovec 后复制用户数据进 pipe；支持部分成功和非阻塞上限，暂不赠送用户物理页 |
| `copy_file_range` | 285 | 普通文件间复制；支持独立输入/输出 offset、同 inode 重叠检查、短写和部分成功语义 |
| `ioprio_set/get` | 30 / 31 | TCB 保存线程级编码，支持 process/pgrp/user 目标、权限检查及 fork/clone 继承 |
| `timerfd_create/settime/gettime` | 85–87 | 支持 realtime/monotonic、绝对/相对超时、周期累计、非阻塞、poll、dup/fork 共享状态与 CLOEXEC |
| `signalfd4` | 74 | 支持创建/更新 mask、批量读取、非阻塞、poll、dup/fork 共享；用户复制失败时按原 thread/process pending 归属回滚 |
| `recvmmsg` | 243 | 支持批量 UDP/TCP 消息接收、每条 `msg_len`、超时、`MSG_WAITFORONE` 和部分成功语义 |
| `memfd_create` | 279 | 内存文件、共享 mmap、truncate、CLOEXEC、seals 与可写映射冲突检查 |
| `inotify_*` | 26–28 | watch 生命周期、文件/目录变更、rename cookie、poll 与 EFAULT 读取回滚 |
| `openat2` | 437 | 版本化 open_how、CLOEXEC、NO_SYMLINKS/BENEATH；CACHED 明确回退 |
| futex bitset/requeue | 98 | 非零 bitset 选择唤醒，requeue 同步 scheduler 队列与 futex waiter 归属 |
| `pidfd_*` | 424/434/438 | open、signal、getfd、poll 与 `waitid(P_PIDFD)` |
| `mincore` / `mlock` | 232/228 | 真实 PTE 驻留查询、区间验证与缺页预取；无 swap 模型下保持驻留 |

同时修正了已有实现中的错误语义：

- `fallocate(FALLOC_FL_KEEP_SIZE)` 在确实需要预分配、但 VFS 没有该能力时改为
  `EOPNOTSUPP`，不再“什么都没做却返回成功”。
- `mount`、`umount2` 增加 euid 0 权限检查；能力系统/用户命名空间尚未实现，
  因此目前以 root 近似 `CAP_SYS_ADMIN`。
- 为未实现调用补齐统一的 asm-generic 编号，方便分发表和审计工具发现缺口。
- 修正文档中的旧编号：`syncfs=267`、`setns=268`、`finit_module=273`。
- `sendfile` 改用 VFS read lease；输出短写或后续错误时返回已完成字节，未交付
  数据不会被错误地从输入流消费。
- `signalfd` 读取使用 pending 事务：先按原作用域预留信号，完成整条 128 字节
  `signalfd_siginfo` 用户复制后才提交；`EFAULT`、分配失败或 lease 丢弃均恢复 pending。

## 2. 已登记但尚未分发的 syscall

以下 16 个编号会稳定落入分发表的 `ENOSYS` 兜底。它们不是简单加一个函数就能
正确实现；必须先有对应内核状态和生命周期管理。

### 2.1 根目录与命名空间

| syscall | nr | 所需基础设施 |
|---|---:|---|
| `pivot_root` | 41 | mount namespace、旧根/新根引用、cwd/root 一致性与并发卸载 |
| `chroot` | 51 | per-process root、所有 `*at` 路径解析的根边界、fork/exec 继承 |
| `setns` | 268 | namespace fd、引用计数和各类 namespace 的切换规则 |

`chroot` 不能仅保存一个字符串；否则 `..`、绝对路径、已打开 dirfd 和符号链接都
可能逃出新根目录。

### 2.2 I/O 调度与交换区

| syscall | nr | 所需基础设施 |
|---|---:|---|
| `swapon` / `swapoff` | 224 / 225 | swap area、换入换出、反向映射、页回收和并发失效 |

`ioprio` 现已具有真实的 per-task 状态、权限、查询和继承语义，但 WaterOS 的块 I/O
仍是同步提交，没有可按优先级重排的异步 elevator。因此它不会被错误地用于 CPU
调度，后续块层引入请求队列时再消费 `TaskSnapshot::io_priority`。

### 2.3 内核模块

| syscall | nr | 所需基础设施 |
|---|---:|---|
| `init_module` | 105 | relocatable ELF 装载、符号解析、权限和模块生命周期 |
| `delete_module` | 106 | 引用计数、资源撤销与并发卸载 |
| `finit_module` | 273 | fd 装载、签名/参数处理，并复用模块装载核心 |

WaterOS 当前采用静态链接组件架构，因此这组暂列低优先级。

### 2.4 SysV IPC

| syscall | nr | 所需基础设施 |
|---|---:|---|
| `msgget/msgctl/msgrcv/msgsnd` | 186–189 | 消息队列 registry、权限、阻塞、删除唤醒 |
| `semget/semctl/semtimedop/semop` | 190–193 | 信号量组、`SEM_UNDO`、超时和进程退出清理 |

这组会影响 BusyBox `ipcs/ipcrm`，部分 `syslogd/logread` 配置也会依赖 SysV
信号量。WaterOS 已有的 SysV SHM（194–197）不能替代 msg/sem。

## 3. 已分发但仍需完善的实现

### P0：正确性或安全性

- `getrandom` 当前是基于 tick、地址和 tid 的伪随机数，不具备密码学
  安全性。需要平台熵源、全局 CSPRNG、初始化状态以及 `GRND_*` 的阻塞语义。
- `sysinfo` 的 total/free RAM 与进程数已接 frame allocator 和 process registry；
  load average、shared/buffer RAM 仍需要 scheduler 与页缓存统计。
- `fallocate` 仅能用 `truncate` 实现 mode 0 的文件扩展，洞打孔、真正预分配和
  `KEEP_SIZE` 尚无后端。
- `mount/umount2/reboot/sethostname` 目前使用 euid 0 代替 capability 检查；加入
  user namespace 后必须改为 `CAP_SYS_ADMIN/CAP_SYS_BOOT/CAP_SYS_CHROOT` 等检查。

### P1：兼容性与性能

- `readahead` 与 `fadvise64` 当前只校验并接受提示，没有异步预读队列。
- `rseq` 明确返回 `ENOSYS`；glibc 能回退，但线程运行时性能路径不完整。
- futex 已覆盖 wait/wake、任意非零 bitset、requeue/cmp_requeue 和 robust 路径；
  PI 与 `FUTEX_WAKE_OP` 仍返回 `ENOSYS`，需要按 LTP 用例逐项补齐。
- `statfs/sysinfo/getrusage/times` 中部分统计仍是近似值，应避免把占位统计当成
  精确内核计数。
- `timerfd` 尚不支持 `TFD_TIMER_CANCEL_ON_SET`。当前 VFS 错误接口没有
  `ECANCELED`，因此显式返回 `EINVAL`，没有伪装为成功。
- `signalfd_siginfo` 当前可靠填写 `signo/pid/uid`；普通信号仍遵循位集合并语义，
  不累计同号普通信号。实时信号排队及完整 `siginfo` 来源字段仍需单独建设。

`rt_sigreturn(139)` 虽然在普通分发表中显示为 `ENOSYS`，但已经由
`os/src/trap_handler.rs` 在进入普通分发前调用 `restore_signal_frame` 接管；这不是
缺失实现。若未来合并 trap 入口，必须保留该特殊顺序。

## 4. 现代 Linux 能力状态

当前 BusyBox 主路径未必需要，但 Debian、apt、编译工具和较新的 libc 会逐步触发：

- 文件搬运：`splice`、`tee`、`vmsplice`、`copy_file_range` 已完成。
- fd/事件：`timerfd_*`、`signalfd4`、`inotify_*`、`memfd_create` 已完成。
- 网络：`recvmmsg` 已完成，当前复用现有 IPv4 TCP/UDP `recvmsg` 后端。
- 进程：`pidfd_open`、`pidfd_send_signal`、`pidfd_getfd` 与 `waitid(P_PIDFD)` 已完成；
  `CLONE_PIDFD` 仍需纳入 clone 的完整回滚事务。
- 路径安全：`openat2` 已完成主要约束；`RESOLVE_IN_ROOT` 在 per-process root
  resolver 完成前返回 `EOPNOTSUPP`，`RESOLVE_CACHED` 在无 dcache-only 路径时返回 `EAGAIN`。
- 内存驻留：`mincore`、`MADV_POPULATE_*` 和区间 mlock 已有真实页表语义；
  `MCL_CURRENT` 等待全 VMA 枚举和锁页计费。

建议先通过真实用户程序或 LTP 日志收集 syscall nr，再按“有完整后端语义”的原则
实现；不要为了让命令表面成功而增加空操作桩。

## 5. 验证方式

本轮提供三层验证：

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre

# 构建含目标机测试程序的用户镜像
make -C ../user image ARCH=rv PACKAGE=operator

# 在 WaterOS shell 内执行
wos-syscall-smoke
```

同一测试程序也已用 `ARCH=la PACKAGE=operator` 构建并在 LoongArch QEMU 中执行，
RISC-V 已在 SMP=2 QEMU 中输出 14 组 `PASS`；LoongArch 当前批次已完成交叉
编译检查，目标机镜像回归使用相同测试源。

`wos-syscall-smoke` 直接发出 asm-generic syscall 号，校验：

- `copy_file_range` 的文件内容、输入/输出 offset 及非法 flags。
- `sendfile` 的显式 offset 隔离与顺序 offset 推进。
- `splice` 的 file→pipe→file 数据和 offset，以及“两端都不是 pipe”的 `EINVAL`。
- `tee` 复制后输入未消费，`vmsplice` 多 iovec 内容顺序正确。
- `ioprio` 的 set/get 和 fork 继承。
- `timerfd` 的非阻塞读、gettime、poll、周期超时累计以及 dup 共享状态。
- `recvmmsg` 的真实 loopback UDP 批量数据、每条长度和无数据超时。
- `signalfd4` 的 mask、poll、批量读取、来源字段、mask 更新，以及故意触发
  `EFAULT` 后 pending 不丢失。
- memfd seal/共享映射、inotify 事件与 rename cookie、openat2 约束。
- futex bitset 定向唤醒以及 requeue 后从目标地址继续定向唤醒。
- pidfd open/getfd/signal/poll/waitid 的完整进程监管链路。
- mincore 驻留位、MADV_POPULATE、非页对齐 mlock 与 MCL_FUTURE。

纯 Rust 边界断言同时接入 `syscall/self_test`，可随内核 self-test 构建在目标架构
执行。直接在 x86_64 宿主运行
`cargo test -p wateros-syscall-impl-kernel` 会先编译 RISC-V `sbi-rt`，因宿主没有
`a0/a7` 寄存器而失败；这属于现有测试基础设施限制，后续应为纯 ABI 逻辑拆出
host-test crate，不能把该失败误判为 syscall 实现错误。

## 6. 现场优先级（2026）

结合往届线下题和当前用户空间，后续按以下顺序推进：

1. **P0 安全随机**：接 VirtIO RNG/开发板熵源与统一 CSPRNG，替换当前不可用于
   TLS 密钥的伪随机实现。
2. **P0 进程/路径兼容**：把 `CLONE_PIDFD` 纳入 clone 回滚事务；真实 `chroot`
   需要 per-process root + `*at`/symlink/dirfd 全链路约束。
3. **P1 现代运行时**：补 futex WAKE_OP/PI、queued realtime signal、复杂 socket
   ancillary data；rseq 在具备迁移 hook 前继续明确 `ENOSYS`。
4. **P1 可观测性**：真实 `sysinfo/statfs/getrusage/times` 计数、`/proc` 配套字段。
5. **P2 专用设施**：SysV msg/sem、namespace、swap、内核模块；只有题目或真实程序
   命中时再做，保持未实现时明确 `ENOSYS`。
