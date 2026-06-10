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
| `read` / `write` / `close` | 部分 | VFS fd；stdin 仍 `EBADF` |
| `pread64` / `pwrite64` / `preadv` / `pwritev` | 部分 | `VfsIoHandle::read_at`/`write_at`；pipe/socket → `ESPIPE` |
| `sendfile` | 部分 | 文件→文件/socket；内核 64KiB 缓冲循环；`offset*` 可选 |
| `ppoll` (73) | 部分 | 共享 `poll_engine`；pipe waitqueue 阻塞；`sigmask` 首期忽略 |
| `pselect6` (72) / `select` (23) | 部分 | `fd_set` 扫描；`select` 不写回剩余 `timeval` |
| `poll` (271) | 部分 | 同引擎；`timeout` 为毫秒 |
| `openat` | 部分 | `AT_FDCWD`、目录 fd、`O_DIRECTORY` |
| `faccessat` (48) / `faccessat2` (439) | 部分 | 48 忽略 a3（Linux 三参 ABI）；439 经 `dispatch_unknown`；`AT_EACCESS`/`AT_EMPTY_PATH`；symlink nofollow 待 VFS |
| `dup` / `dup3` | 已接入 | `vfs::fd::dup_fd` / `dup3_fd` |
| `pipe2` | 部分 | 创建 pipe fd；fork 后 `copy_fd_table_from_parent` |
| `fstat` / `lseek` | 部分 | 128B `kstat`；pipe `ESPIPE` |
| `getdents64` | 部分 | `linux_dirent64`；目录须先 `O_DIRECTORY` open |
| `mkdirat` / `unlinkat` | 部分 | 仅 `AT_FDCWD`；RO 辅助卷写返回 `EROFS` |
| `mount` | 部分 | `MS_RDONLY` → `mount_ro` 辅助卷；否则 `mount_rw` |
| `umount2` | 部分 | `vfs::unmount_at` |
| `brk` / `mmap` / `munmap` / `mprotect` | 部分 | Sv39 `user_aspace_ptr` |
| `get_mempolicy` (236) | 部分 | 单节点 stub：`MPOL_DEFAULT` + nodemask node 0；`MPOL_F_ADDR` 映射校验 |
| `sched_getaffinity` (123) | 部分 | 单核 stub：CPU 0 mask，返回 8；无 `sched_setaffinity` |
| `clone`（含 `fork`） | 部分 | `fork_user_aspace` + 子进程保留父 `user_sp`；继承 cwd/fd |
| `execve` | 部分 | 替换地址空间/入口/栈；非 ELF 文本脚本经 shebang 解析后加载解释器 ELF |
| `waitpid` | 部分 | 最小父子等待、`WNOHANG` |
| `getpid` / `getppid` / `gettid` | 部分 | orphan ppid 为 1 |
| `getuid` / `geteuid` / `getgid` / `getegid` / `getgroups` | 已接入 | 经 `wateros-cred`；`getgroups` G1 返回 `[0]` |
| `setuid` / `setgid` / `setreuid` / `setregid` / `setresuid` / `setresgid` | 已接入 | impl-root 放行 ID 更新；非法超宽 uid/gid 返回 `EINVAL` |
| `gettimeofday` / `clock_settime` / `clock_gettime` / `clock_getres` / `clock_nanosleep` / `times` / `nanosleep` | 部分 | `platform::timer` 单调时钟 + REALTIME offset；sleep 精度 ~10ms |
| `getcwd` / `chdir` | 部分 | per-task cwd |
| `uname` | 部分 | 固定 `utsname` 字段 |
| `syslog` (116) | **已接入** | `sys_syslog` → **`wateros-klog`**；传统 ASCII 读路径；见 [`docs/architecture/wateros-klog.md`](../../architecture/wateros-klog.md) |

## 明确未覆盖

- 完整 Linux syscall 面、`clone` flags、完整 `mount` flags（`MS_BIND` 等）。
- `#!/usr/bin/env` + PATH 搜索（首版 shebang 仅支持解释器路径直写）。
- 用户缓冲严格 `copy_from_user` / `copy_to_user`。
- signal、futex、完整 `fcntl`/`ioctl` 等 busybox 后续项。

## 维护要求

分发表或 `sys_*` 行为变化时，同步更新本文件与 **`os/components/wateros-syscall/TODO.md`**。
