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

同时修正了已有实现中的错误语义：

- `fallocate(FALLOC_FL_KEEP_SIZE)` 在确实需要预分配、但 VFS 没有该能力时改为
  `EOPNOTSUPP`，不再“什么都没做却返回成功”。
- `mount`、`umount2` 增加 euid 0 权限检查；能力系统/用户命名空间尚未实现，
  因此目前以 root 近似 `CAP_SYS_ADMIN`。
- 为未实现调用补齐统一的 asm-generic 编号，方便分发表和审计工具发现缺口。
- 修正文档中的旧编号：`syncfs=267`、`setns=268`、`finit_module=273`。

## 2. 已登记但尚未分发的 syscall

以下 18 个编号会稳定落入分发表的 `ENOSYS` 兜底。它们不是简单加一个函数就能
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
| `ioprio_set` / `ioprio_get` | 30 / 31 | per-task I/O priority 状态以及块 I/O 调度器实际消费 |
| `swapon` / `swapoff` | 224 / 225 | swap area、换入换出、反向映射、页回收和并发失效 |

在块层真正消费 ioprio 之前，不应添加永远返回成功的 `ionice` 桩。

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

- `getrandom` 当前是基于 tick、地址和 tid 的 xorshift 伪随机数，不具备密码学
  安全性。需要平台熵源、全局 CSPRNG、初始化状态以及 `GRND_*` 的阻塞语义。
- `sysinfo` 的 `freeram=totalram/2`、`procs=1`、load average 全零仍是占位值；应接
  frame allocator、process registry 和 load accounting。
- `fallocate` 仅能用 `truncate` 实现 mode 0 的文件扩展，洞打孔、真正预分配和
  `KEEP_SIZE` 尚无后端。
- `mount/umount2/reboot/sethostname` 目前使用 euid 0 代替 capability 检查；加入
  user namespace 后必须改为 `CAP_SYS_ADMIN/CAP_SYS_BOOT/CAP_SYS_CHROOT` 等检查。

### P1：兼容性与性能

- `readahead` 与 `fadvise64` 当前只校验并接受提示，没有异步预读队列。
- `rseq` 明确返回 `ENOSYS`；glibc 能回退，但线程运行时性能路径不完整。
- futex 已覆盖常用 wait/wake 路径，但部分 PI、requeue/bitset 组合仍返回
  `ENOSYS`，需要按 LTP 用例逐项补齐。
- `statfs/sysinfo/getrusage/times` 中部分统计仍是近似值，应避免把占位统计当成
  精确内核计数。

`rt_sigreturn(139)` 虽然在普通分发表中显示为 `ENOSYS`，但已经由
`os/src/trap_handler.rs` 在进入普通分发前调用 `restore_signal_frame` 接管；这不是
缺失实现。若未来合并 trap 入口，必须保留该特殊顺序。

## 4. 待纳入编号表的现代 Linux 能力

当前 BusyBox 主路径未必需要，但 Debian、apt、编译工具和较新的 libc 会逐步触发：

- 文件搬运：`splice`、`tee`、`vmsplice`、`copy_file_range`。
- fd/事件：`timerfd_*`、`signalfd4`、`inotify_*`、`memfd_create`。
- 网络：`recvmmsg`。
- 进程：`pidfd_open`、`pidfd_send_signal`、`pidfd_getfd`。
- 路径安全：`openat2`。

建议先通过真实用户程序或 LTP 日志收集 syscall nr，再按“有完整后端语义”的原则
实现；不要为了让命令表面成功而增加空操作桩。

## 5. 验证方式

本轮已经执行：

```bash
cd os
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

两种架构均通过。纯 Rust 单元断言同时接入 `syscall/self_test`，可随内核 self-test
构建在目标架构执行。直接在 x86_64 宿主运行
`cargo test -p wateros-syscall-impl-kernel` 会先编译 RISC-V `sbi-rt`，因宿主没有
`a0/a7` 寄存器而失败；这属于现有测试基础设施限制，后续应为纯 ABI 逻辑拆出
host-test crate，不能把该失败误判为 syscall 实现错误。
