# PC 用户态文件系统兼容性：syscall 与 flag 任务清单

日期：2026-08-14
状态：代码修复已完成；每进程 root、符号链接、`openat/openat2` flag 校验与
`O_TMPFILE` 链路已通过两架构静态构建，按交付约定尚未运行用户态回归。

## 目标与边界

目标是让 WaterOS 能稳定运行常规 RISC-V Linux 用户程序，包括 BusyBox、动态链接程序、
编辑器、开发工具、Arch RISC-V `pacman` 的隔离 root 安装，以及常见桌面应用的文件操作。

本清单**不以容器运行时或完整 Linux mount namespace 兼容为目标**。对暂不支持的功能，
正确策略是返回 Linux 预期的 `EINVAL`、`EOPNOTSUPP` 或 `ENOSYS`，不能静默忽略 flag
或伪造成功。

## 已接入的基础 syscall

以下 syscall 已在 WaterOS 分发表中接入；后续任务主要是补全其 flag 语义与回归测试：

```text
openat/openat2                readlinkat
faccessat/faccessat2          fchdir
fchmod/fchmodat               fchown/fchownat
fstat/fstatat/statx           statfs/fstatfs
getdents64                    mkdirat/mknodat
symlinkat/unlinkat/linkat     renameat/renameat2
utimensat                     mount/umount2
fsync/fdatasync/sync/syncfs   truncate/ftruncate/fallocate
fcntl/flock                   set/get/list/remove xattr
inotify_init1/add_watch/rm_watch
```

已确认未接入：`pivot_root(2)`。`chroot(2)` 已接入并共享统一的进程 root resolver。

## P0：必须完成，或在未完成前显式拒绝

### F-01：每进程 root 与 `chroot(2)`

这是当前 Arch RISC-V 软件包隔离安装与动态程序运行的首要阻塞项。

实现状态：`chroot(2)` 已接入分发；root 与 cwd 共用 fork/`CLONE_FS` 生命周期，exec 保留，
退出回收。绝对路径、相对路径、dirfd、`..` 与符号链接展开统一约束在 root 内，`getcwd`
返回 root 内逻辑路径。当前选择在 `chroot` 成功时同步把 cwd 调整到新 root。尚待用户态验证。

要求：

1. 新增 `chroot(2)` syscall 分发与实现。
2. root 是进程属性；`fork/clone` 继承，`execve` 保持，进程退出时回收。
3. 所有绝对路径从该进程 root 解析；不能继续固定从全局 `/` 开始。
4. cwd、`dirfd`、`openat/openat2`、`*at`、`getcwd`、`execve`、symlink 跟随和
   `/proc/<pid>/...` 相关路径必须使用同一 resolver。
5. 禁止经 `..`、绝对 symlink 或保存的 dirfd 逃逸新 root。
6. `chroot` 成功后应将 cwd 调整为新 root，或按 Linux 语义要求调用方 `chdir("/")`；
   无论选择哪种，必须有测试并保持路径约束安全。

验收：

```sh
chroot /opt/archriscv /usr/bin/nvim --version
chroot /opt/archriscv /usr/bin/tree /
```

### F-02：符号链接完整回归

`symlinkat` 已修复，但必须验证创建、读取、删除、重命名、metadata 和持久化是一个整体。

```sh
ln -s target link
readlink link
test -L link
cat link
ln -s missing dangling
test -L dangling
test ! -e dangling
```

同时覆盖 `unlinkat`、`renameat2`、`lstat/fstatat(AT_SYMLINK_NOFOLLOW)`、循环返回
`ELOOP`、重启后 `readlink`，并在镜像副本上执行宿主 `e2fsck -fn`。

### F-03：`openat(2)` flag 审计与拒绝策略

当前 `openat` 不应默默吞掉任何未实现的状态 flag。建立集中校验表：

实现状态：`openat` 和 `openat2` 共用完整的合法位集；未知位返回 `EINVAL`，
`O_SYNC/O_DSYNC/O_DIRECT/O_ASYNC/O_NOATIME` 明确返回 `EOPNOTSUPP`。

| flag | 目标 | 任务要求 |
|---|---|---|
| `O_RDONLY/O_WRONLY/O_RDWR` | 必须支持 | 已有，回归读写权限 |
| `O_CREAT/O_EXCL/O_TRUNC` | 必须支持 | 已有，验证与 symlink/目录组合 |
| `O_APPEND` | 必须支持 | 验证并发/多 fd 追加语义 |
| `O_CLOEXEC` | 必须支持 | 已有，验证 exec 后 fd 关闭 |
| `O_DIRECTORY` | 必须支持 | 非目录必须 `ENOTDIR` |
| `O_NOFOLLOW` | 必须支持 | 已有，末端 symlink 必须 `ELOOP` |
| `O_NONBLOCK` | 必须支持 | 对 pipe/socket 生效；普通文件可无影响 |
| `O_PATH` | 建议支持 | 必须配合 `fstatat(AT_EMPTY_PATH)`、`fchdir` 回归 |
| `O_NOCTTY` | 已涉及 PTY | 保持现有 PTY 行为回归 |
| `O_LARGEFILE` | 必须接受 | 64 位 libc 常带此位，不能误判为 `O_NOFOLLOW` |
| 未知 bit | 必须拒绝 | 返回 `EINVAL` |

### F-04：同步/直接 I/O flag 不得伪实现

| flag | 当前策略要求 |
|---|---|
| `O_SYNC` / `O_DSYNC` / `O_RSYNC` | 实现每次写入的持久化语义，或明确 `EOPNOTSUPP` |
| `O_DIRECT` | 实现绕过页缓存与对齐约束，或明确 `EOPNOTSUPP` |
| `O_TMPFILE` | 实现匿名 inode、`linkat(AT_EMPTY_PATH)` 公开与 close 回收，或明确 `EOPNOTSUPP` |

特别说明：目前 `O_TMPFILE` 若以目录中的可见 `.wateros-tmpfile-*` 替代，不能视为 Linux
兼容实现；它可能遗留文件且不具备匿名 inode 语义。

当前实现不再创建可见替代文件：ext4 与 tmpfs 后端直接创建 `nlink=0` 的稳定节点，fd
持有其生命周期；最后一个 fd 关闭时回收，或通过 `linkat(AT_EMPTY_PATH)` 在同一挂载中
发布。`O_TMPFILE | O_EXCL` 创建的节点禁止发布。该结论仅完成静态构建验证。

### F-05：flag 错误码一致性

统一约束：

```text
未知/非法 flag 位             -> EINVAL
已知但该对象或后端不支持      -> EOPNOTSUPP
只读文件系统的写操作          -> EROFS
末端 symlink 被禁止跟随       -> ELOOP
目录作为普通文件打开/写入     -> EISDIR 或 ENOTDIR（按 syscall 语义）
```

不要把 `VfsError::Unsupported` 一律映射成 `EINVAL`；各 syscall 应依据 Linux 语义转换。

## P1：常见桌面、工具链和包管理程序的兼容性

### F-06：`*at` family flag 组合

为下列 syscall 建立 symlink、空路径、dirfd、权限和错误码回归：

| syscall | 需要覆盖的 flag |
|---|---|
| `fstatat` / `statx` | `AT_SYMLINK_NOFOLLOW`、`AT_EMPTY_PATH` |
| `faccessat2` | `AT_EACCESS`、`AT_SYMLINK_NOFOLLOW`、`AT_EMPTY_PATH` |
| `fchmodat` / `fchownat` | `AT_SYMLINK_NOFOLLOW`；不支持组合必须明确拒绝 |
| `linkat` | `AT_SYMLINK_FOLLOW`、`AT_EMPTY_PATH` |
| `unlinkat` | `AT_REMOVEDIR` |
| `utimensat` | `AT_SYMLINK_NOFOLLOW`、`AT_EMPTY_PATH` |

### F-07：`openat2(2)` 安全 flag

实现状态：`RESOLVE_IN_ROOT` 使用 dirfd 作为临时 root，绝对路径、`..` 和绝对
symlink 都不能逃逸；它与 `RESOLVE_BENEATH` 的非法组合返回 `EINVAL`。
`RESOLVE_CACHED` 仍因没有纯 dcache 路径而返回 `EAGAIN`。

| flag | 建议 |
|---|---|
| `RESOLVE_NO_SYMLINKS` | 保持支持并回归 |
| `RESOLVE_BENEATH` | 保持支持；不能因 `..` 或 symlink 逃逸 dirfd |
| `RESOLVE_NO_XDEV` | 保持支持，跨挂载返回 `EXDEV` |
| `RESOLVE_NO_MAGICLINKS` | `/proc` magic link 语义未完整时可等同普通路径，但需记录 |
| `RESOLVE_IN_ROOT` | 已实现 dirfd 内 root 语义 |
| `RESOLVE_CACHED` | 暂缓；无纯 dcache 路径时正确返回 `EAGAIN` |

### F-08：`fcntl` 与文件锁

已实现的 `F_DUPFD`、`F_DUPFD_CLOEXEC`、`F_GETFD/F_SETFD`、`F_GETFL/F_SETFL`、POSIX
record locks、pipe size 与 memfd seals 应回归。普通 PC 程序还应保证：

- `F_SETFL` 只改变 Linux 允许改变的状态位；
- `O_APPEND`、`O_NONBLOCK` 的返回值和实际行为一致；
- 没有 `O_DIRECT` 实现时，不得报告它已生效；
- 未识别 command 返回 `EINVAL`，坏 fd 返回 `EBADF`。

OFD locks、`F_SETOWN/F_GETOWN`、`F_SETSIG`（SIGIO）、lease（`F_SETLEASE`）可延后。

本次已将 `F_SETFL` 限制为只修改 `O_APPEND/O_NONBLOCK`；尝试设置
`O_DIRECT/O_ASYNC/O_NOATIME` 返回 `EOPNOTSUPP`。

### F-09：安装/更新路径与持久化

对 pacman 的实际写入模式建立专用回归：

```text
mkdir -> 写临时文件 -> fsync/fdatasync -> renameat2 -> fsync 父目录
创建/替换 symlink
文件权限、owner、mtime
xattr 不支持时的明确错误
卸载/重启后数据、目录项、链接计数仍正确
```

建议使用干净 `/opt/archriscv`：

```sh
archriscv-pacman -S tree
chroot /opt/archriscv /usr/bin/tree /
archriscv-pacman -S neovim
chroot /opt/archriscv /usr/bin/nvim --version
```

## P2：可延后，不阻塞普通 PC 用户程序

以下属于容器、overlayfs、专用服务器或性能优化范围，不是当前 pacman/mGBA/桌面目标的
前置条件：

```text
pivot_root
mount namespace / fsopen / fsconfig / fsmount / move_mount / mount_setattr
renameat2(RENAME_WHITEOUT)
openat2(RESOLVE_CACHED)
O_ASYNC、F_SETOWN/F_GETOWN、F_SETSIG、SIGIO
OFD locks、文件 lease
完整 O_DIRECT
更完整的 ext4 崩溃孤儿 inode 恢复
完整 ACL、capability xattr、fanotify、file-handle API
```

其中 `pivot_root` 已有 syscall 号但尚未接入分发；在没有容器目标前保持 `ENOSYS` 即可。

## 当前推荐的实施顺序

```text
F-01 per-process root + chroot
  -> F-02 symlink 全链路回归
  -> F-03 openat flag 校验
  -> F-05 错误码统一
  -> F-06 *at flag 回归
  -> F-09 pacman 端到端持久化
  -> F-04 同步/直接 I/O 与 O_TMPFILE 语义
  -> F-07/F-08/P2 按实际 workload 决定
```

## 验收原则

每个任务都需要：

1. 最小用户态复现程序或 BusyBox 命令；
2. 错误路径和 flag 组合测试；
3. RISC-V 内核构建与 QEMU 实测；
4. 涉及写入时用镜像副本/overlay，并在宿主执行 `e2fsck -fn`；
5. 禁止通过用户态 wrapper 改写绝对路径、忽略 flag 或用 `--overwrite` 掩盖内核问题。
