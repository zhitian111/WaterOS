# wateros-syscall 功能快照

## 用途

记录 **`wateros-syscall`** 一级组件在默认构建下的系统调用分发与用户态契约对接范围，便于与 **`wateros-abi`**、**`wateros-task`**、**`wateros-vfs`**、平台 trap 路径对照。

## 事实来源

- `os/components/wateros-syscall/Cargo.toml`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/`
- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/Cargo.toml`（根依赖 `syscall`）

## 聚合层与依赖

- 根 crate **`wateros`** 依赖 **`wateros-syscall`**（包名 `syscall`），平台 feature 启用 **`impl-riscv64`** / **`impl-loongarch64`**。
- **`fd-session`** 下依赖 **`wateros-vfs`**：`vfs::fd` per-task fd 表、`vfs::cwd`、`vfs::mount_ext4_block_at`。
- **`cred-session`** 下依赖 **`wateros-cred`**：identity syscall 读取/更新当前任务 `ProcessCredentials`。
- **`read` / `write` / `close` / `pipe2` / `openat` / `getdents64` / `unlinkat` / `mkdirat`** 等经 **`vfs_util::vfs_error_to_errno`** 映射 **`VfsError`**（含 **`ReadOnlyFs` → `EROFS`**）。

## 当前已具备能力（basic bring-up 相关）

| 能力 | 状态 | 要点 |
|------|------|------|
| `yield` / `exit` / `exit_group` | 已接入 | `task::yield_now` / `exit_current` |
| `read` / `write` / `close` | 部分 | VFS fd；stdin 无真实输入时多为 **EOF(0)** |
| `pread64` / `pwrite64` / `preadv` / `pwritev` | 部分 | `VfsIoHandle::read_at`/`write_at`；pipe/socket → `ESPIPE` |
| `sendfile` | 部分 | 文件→文件/socket；内核 64KiB 缓冲循环；`offset*` 可选 |
| `ppoll` (73) | 部分 | 共享 `poll_engine`；pipe waitqueue 阻塞；**`sigmask` 阻塞期间临时应用** |
| `pselect6` (72) | 部分 | `fd_set` 扫描；**`sigmask` 同 ppoll** |
| `poll` (271) | 部分 | 同引擎；`timeout` 为毫秒 |
| `openat` | 部分 | `AT_FDCWD`、目录 fd、`O_DIRECTORY`；**follow 末端 symlink**；`O_NOFOLLOW`/`O_SYNC`/`O_DSYNC` 显式处理 |
| `faccessat` (48) / `faccessat2` (439) | 部分 | 48 忽略 a3（Linux 三参 ABI）；439 经 `dispatch_unknown`；`AT_EACCESS`/`AT_EMPTY_PATH`；symlink nofollow 待 VFS |
| `dup` / `dup3` | 已接入 | `vfs::fd::dup_fd` / `dup3_fd` |
| `pipe2` | 部分 | 创建 pipe fd；fork 后 `copy_fd_table_from_parent` |
| `fstat` / `lseek` | 部分 | 128B `kstat`；pipe `ESPIPE` |
| `getdents64` | 部分 | `linux_dirent64`；目录须先 `O_DIRECTORY` open |
| `mkdirat` / `unlinkat` | 部分 | 仅 `AT_FDCWD`；RO 辅助卷写返回 `EROFS` |
| `mount` | 部分 | `MS_RDONLY`/`MS_REMOUNT`；拒绝 `MS_BIND`/`MS_SHARED` 等 |
| `umount2` | 部分 | `vfs::unmount_at` |
| `brk` / `mmap` / `munmap` / `mprotect` | 部分 | 需 `user_aspace_ptr`；无则 `-ENOSYS` |
| `get_mempolicy` (236) | 部分 | 语义在 `wateros-mm::mempolicy` |
| `sched_setparam` (118)–`sched_getaffinity` (123) | 部分 | 语义在 `wateros-task::sched`；set RT/affinity → `EPERM` |
| `clone`（含 `fork`/`vfork` 兼容） | 部分 | leader-only fork；普通 fork 接受 `CSIGNAL` 与 parent/child tid flags；`CLONE_VM\|CLONE_VFORK\|CLONE_CLEAR_SIGHAND\|CSIGNAL` 降级为普通 fork |
| `execve` | 部分 | 替换地址空间/入口/栈；非 ELF 文本脚本经 shebang 解析后加载解释器 ELF |
| `waitpid` | 部分 | 最小父子等待、`WNOHANG` |
| `getpid` / `getppid` / `gettid` | 部分 | orphan ppid 为 1 |
| `getuid` / `geteuid` / `getgid` / `getegid` / `getgroups` | 已接入 | 经 `wateros-cred`；`getgroups` G1 返回 `[0]` |
| `setuid` / `setgid` / `setreuid` / `setregid` / `setresuid` / `setresgid` | 已接入 | impl-root 放行 ID 更新；非法超宽 uid/gid 返回 `EINVAL` |
| `gettimeofday` / `clock_settime` / `clock_gettime` / `clock_getres` / `clock_nanosleep` / `times` / `nanosleep` | 部分 | `platform::timer` 单调时钟 + REALTIME offset；sleep 精度 ~10ms |
| `getcwd` / `chdir` | 部分 | per-task cwd |
| `uname` | 部分 | 固定 `utsname` 字段 |
| `syslog` (116) | **已接入** | `sys_syslog` → **`wateros-klog`**；传统 ASCII 读路径；见 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md) |
| `socketpair` (199) | 部分 | `AF_UNIX` + `SOCK_STREAM`；VFS 双 pipe 交叉；BusyBox shell IPC |

## 语义审计与已收敛项（2026-06-25）

完整问题清单与覆盖说明见 [`docs/audits/syscall-issues.md`](../../audits/syscall-issues.md)、[`docs/audits/syscall-coverage.md`](../../audits/syscall-coverage.md)。

**本轮已收敛（明确拒绝，不再 panic/静默走错路径）**：

| syscall | 条件 | 行为 |
|---------|------|------|
| `mmap`/`munmap`/`mprotect`/`mremap` | 无 `user_aspace_ptr` | `warn` + `-ENOSYS` |
| `MmError::Unsupported` | mm 层不支持操作 | `-ENOSYS`（非 panic） |
| `clone`/`fork` | 非 leader 线程 fork | `warn` + `-EPERM` |
| `clone`/`fork` | fork 路径 flags 超出 `CSIGNAL` 与 parent/child tid flags（除 `CLONE_VM\|CLONE_VFORK` 兼容形态） | `warn` + `-EINVAL` |
| `clone`/`fork` | `CLONE_PARENT_SETTID` / `CLONE_CHILD_SETTID` / `CLONE_CHILD_CLEARTID` | 写 parent/child tid；子进程退出清零并 futex wake |
| `clone`/`vfork` | `CLONE_VM\|CLONE_VFORK\|CLONE_CLEAR_SIGHAND\|CSIGNAL` | 降级为普通 fork（复制地址空间、不共享 VM、不阻塞父进程） |
| `futex` `WAIT_BITSET`/`WAKE_BITSET` | `bitset != !0` | `warn` + `-ENOSYS` |
| `get_robust_list` | — | 修正为 Linux 三参数 ABI `(pid, head**, len*)` |
| `getgroups` | 非法 size/指针/copy 失败 | `-EINVAL`/`-EFAULT` |
| `syslog` | 空指针读写 | `-EFAULT` |
| `mount` | `MS_BIND`/`MS_SHARED`/`MS_PRIVATE` 等传播 flag | `warn` + `-EINVAL` |
| `execve` | 加载失败 | 不再提前杀兄弟线程（加载成功后再清理） |

**第二轮已收敛（2026-06-25；LTP fast-exit 未动）**：

| syscall | 条件 | 行为 |
|---------|------|------|
| `ppoll`/`pselect6` | `sigmask != NULL` | 阻塞等待期间临时替换线程 mask，返回前恢复 |
| `openat` | 目标为 symlink | follow 至最终路径（`resolve_final_symlink`） |
| `openat` | `O_NOFOLLOW` 且目标为 symlink | `-ELOOP` |
| `openat` | `O_SYNC`/`O_DSYNC` | `warn` + `-EINVAL` |
| `fsync`/`fdatasync` | flush 失败 | `warn` + 对应 errno |
| `read`/`write`/`connect`/`accept`（socket） | 阻塞模式 | 真阻塞；可投递信号 → `EINTR`（去除 tick 上限假 `EAGAIN`/`ETIMEDOUT`） |
| AF_UNIX `accept` | 阻塞模式 | 同上（`socket_blocking_tick`） |

**第三轮已收敛（2026-06-25）**：

| syscall | 行为 |
|---------|------|
| `dup3` | `oldfd==newfd` 且 flags 合法时成功 |
| `pipe2` | 支持 `O_CLOEXEC` |
| `fcntl` | pipe/TTY `F_GETFL`/`F_SETFL` 反映并设置 `O_NONBLOCK` |
| `openat` | `O_CREAT\|O_EXCL` 已存在路径→`-EEXIST` |
| `openat` | 新文件或特殊 devfs 路径 | 不再因 final symlink follow 预检查返回 `ENOENT` |
| `faccessat2` | `AT_SYMLINK_NOFOLLOW` 不 follow |
| `umount2` | 非零 flags→`-EINVAL` |
| `futex` wake | private/shared key 双试 |
| TTY `read` | 无数据非阻塞→`EAGAIN`；阻塞等待输入 |
| `recvfrom` | 与 `read` 相同的阻塞/`EINTR` 语义 |

**第四轮已收敛（2026-06-25；P1 小改）**：

| syscall | 行为 |
|---------|------|
| `brk` | 扩页失败返回 `ENOMEM`/`EINVAL`（不再伪装成功） |
| `clock_settime` | 非 root → `EPERM` |
| `kill` | `pid==0` 当前进程；`pid==-1` 广播（除 self/pid1）；`pid<-1` 按 pgid leader pid 的进程树做 bring-up 兼容 |
| `waitpid` | `pid==0` 等待任意子进程 |
| `execve` | argv/envp 用户指针错误 → `EFAULT` |
| `ioctl` | 未识别 request 打 `warn` |
| `renameat2` | `RENAME_NOREPLACE`；其余 flag → `EINVAL` |
| `getcwd` | 内核路径缓冲 4096 字节 |
| `fallocate` | `KEEP_SIZE` 扩展预分配 stub 成功 |
| robust exit | futex wake 尝试 private + shared key |
| `acct` | root/path 校验 + Linux v0 accounting 记录写入；`acct02` 通过 |
| LTP 环境 | PATH 包含 `testcases/bin`/`testcases/lib`，根布局补齐常用 busybox applet |
| AuxRw 文件 | 普通文件使用 range/paged handle，父进程打开后可观察子进程追加写 |

**文档勘误**：`read`(stdin) 当前多为 **EOF(0)**，非 `EBADF`；用户态 `select` 应走 `pselect6`(72)，nr 23 为 `dup`。

## 明确未覆盖

- 完整 Linux syscall 面、`clone` flags、完整 `mount` flags（`MS_BIND` 等）。
- `#!/usr/bin/env` + PATH 搜索（首版 shebang 仅支持解释器路径直写）。
- 用户缓冲严格 `copy_from_user` / `copy_to_user`。
- signal、futex、完整 `fcntl`/`ioctl` 等 busybox 后续项。

## 维护要求

分发表或 `sys_*` 行为变化时，同步更新本文件与 **`os/components/wateros-syscall/TODO.md`**。
