# 系统调用语义审计：VFS / 路径组（G08–G20）

> 审计范围：VFS 与路径相关 syscall 组 G08–G20  
> Baseline：Linux syscall 语义（riscv64 / asm-generic 64 位号表）  
> 生成时间：2026-06-25  
> 主要源码：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/`、`vfs_util.rs`、`path_at.rs`、`linux_stat.rs`、`stat_times.rs`

---

## 1. 概述

### 1.1 分发入口

| 层级 | 位置 |
|------|------|
| Trap 入口 | `syscall-impl/impl-kernel/src/lib.rs` → `dispatch_syscall_from_trap` |
| 号表分发 | `syscall-api/api-v0` + `abi-impl/impl-linux-generic64` |
| 旁路 nr | `dispatch_unknown`：`fstatat(79)`、`statx(291)`、`faccessat2(439)` |

### 1.2 共享基础设施

| 模块 | 职责 | 可靠性要点 |
|------|------|-----------|
| `sys/path_at.rs` | `AT_FDCWD`/`AT_REMOVEDIR`；`resolve_path_at` | `dirfd<0`→`EBADF`；非目录 fd→`ENOTDIR`；路径经 `vfs::resolve_open_path` / `resolve_against_cwd` |
| `vfs_util.rs` | `VfsError`→`ErrNo`；`linux_open_flags_to_vfs` | 仅映射 `O_ACCMODE/O_CREAT/O_TRUNC/O_APPEND/O_DIRECTORY`；**忽略** `O_EXCL/O_NOFOLLOW/O_NONBLOCK/O_SYNC` 等 |
| `linux_stat.rs` | `struct stat` / `struct statx` 填充 | `st_uid/st_gid` 恒 0；时间戳默认 0（`stat_times` 覆盖） |
| `stat_times.rs` | `utimensat` 写入的 atime/mtime 旁路表 | `spin::Mutex<BTreeMap>`；与 VFS 元数据未打通 |
| `vfs::fd::with_current_io` | 取 fd 句柄执行闭包 | 临时 `take`/`restore` 句柄；与 `poll` 并发有已知竞态（见 `poll_engine.rs` 注释） |

### 1.3 组内 syscall 一览

| nr | 名称 | 入口 | 实现文件 |
|----|------|------|----------|
| 56 | openat | `dispatch_openat` | `sys/openat.rs` |
| 48 | faccessat | `dispatch_faccessat` | `sys/faccessat.rs` |
| 439 | faccessat2 | `dispatch_unknown` 旁路 | `sys/faccessat.rs` |
| 53 | fchmodat | `dispatch_fchmodat` | `sys/fchmodat.rs` |
| 54 | fchownat | `dispatch_fchownat` | `sys/fchownat.rs` |
| 78 | readlinkat | `dispatch_readlinkat` | `sys/readlinkat.rs` |
| 43 | statfs | `dispatch_statfs` | `sys/statfs.rs` |
| 80 | fstat | `dispatch_fstat` | `sys/fstat.rs` |
| 79 | fstatat | `dispatch_unknown` 旁路 | `sys/fstat.rs` |
| 291 | statx | `dispatch_unknown` 旁路 | `sys/fstat.rs` |
| 62 | lseek | `dispatch_lseek` | `sys/lseek.rs` |
| 61 | getdents64 | `dispatch_getdents64` | `sys/getdents64.rs` |
| 34 | mkdirat | `dispatch_mkdirat` | `sys/mkdirat.rs` |
| 36 | symlinkat | `dispatch_symlinkat` | `sys/symlinkat.rs` |
| 35 | unlinkat | `dispatch_unlinkat` | `sys/unlinkat.rs` |
| 276 | renameat2 | `dispatch_renameat2` | `sys/renameat2.rs` |
| 88 | utimensat | `dispatch_utimensat` | `sys/utimensat.rs` |
| 81 | sync | `dispatch_sync` | `sys/sync.rs` |
| 82 | fsync | `dispatch_fsync` | `sys/sync.rs` |
| 83 | fdatasync | `dispatch_fdatasync` | `sys/sync.rs` |
| 46 | ftruncate | `dispatch_ftruncate` | `sys/ftruncate.rs` |
| 47 | fallocate | `dispatch_fallocate` | `sys/fallocate.rs` |
| 40 | mount | `dispatch_mount` | `sys/mount.rs` |
| 39 | umount2 | `dispatch_umount2` | `sys/umount2.rs` |
| 17 | getcwd | `dispatch_getcwd` | `sys/getcwd.rs` |
| 49 | chdir | `dispatch_chdir` | `sys/chdir.rs` |

---

## 2. 横切问题（优先 openat / mount / wait）

### 2.1 P0：openat + 页缓存 + ext4 锁序（卡死风险）

`paged_handle.rs` 头部文档明确：**禁止在持有 ext4 锁后再等待页缓存 entry 锁**；`fsync`/`flush` 路径曾与读 miss 死锁。

- `openat` → `FsBridge::open` → `PagedFileHandle::open` → `global_cache().acquire_open_ref`
- `fsync`/`fdatasync`/`sync` → `handle.flush()` → `cache.flush` → 持 `page_cache` 锁下探 `SharedRwFs`（ext4）
- 并发读写 + flush 时，单核自旋锁长时间占用可表现为**用户态永久阻塞**（无调度点）

**收敛建议**：未验证锁序的路径对 `O_SYNC` 或显式 `fsync` 组合打 `warn!`；短期可对 `FILE_IO_MODE::Async` 直接 `-EOPNOTSUPP`（已实现）。

### 2.2 P0：mount 同步重操作（假死 / 长临界区）

`mount(40)` 块设备路径：

```
sys_mount → vfs::mount_ext4_block_at
  → fs::mount_aux_rw_from_block_path (持 ACTIVE_FS_IMPL Mutex)
    → imp.mount_rw(device) (块设备 Mutex + ext4 探测/加载)
  → mount_table::mount_aux_at_rw (AUX_MOUNTS Mutex)
```

- 全程**同步、无 yield**；`assert_mount_point_directory` 另持 `ROOT_RW_FS` 读元数据
- 错误设备/损坏 superblock 时可能长时间自旋 I/O，测试脚本表现为 hang
- `umount2` **忽略 flags**（`MNT_FORCE`/`MNT_DETACH`），繁忙挂载点无法按 Linux 语义强制卸载

**收敛建议**：

```text
warn!("[syscall] mount(nr=40) flags={:#x} fstype={:?} unsupported flag combination", flags, fstype);
return -EINVAL;
```

对未知 `fstype`（非 ext2/3/4/vfat/tmpfs/proc/cgroup/cgroup2）已在入口 `-EINVAL`；建议对 `MS_BIND`/`MS_SHARED`/`MS_PRIVATE` 等 OR 进来的高位 flag **显式拒绝**而非静默忽略。

### 2.3 P0：LTP wait 协作与 openat/mount 意外退出（wait 相关）

`openat`、`mkdirat`、`mount`、`unlinkat` 入口调用 `cgroup_regression_loop_fast_exit_if_standalone()`（`ltp_cgroup_helper.rs`）：

- 当父进程处于 `TaskBlockReason::Wait` 且子进程为 standalone LTP helper 时，子进程 **`exit(0)` 而非执行真实 syscall**
- 设计意图：避免 LTP worker 阻塞 `wait()` 队列
- **副作用**：真实语义审计时，父 `waitpid` 可能提前收尸，掩盖 open/mount 失败或导致测试逻辑错乱；与「不支持则明确失败」原则冲突

**收敛建议**：用 compile-time feature 或 `prctl` 开关包裹；默认路径打 `warn!` 后走真实 syscall，仅 LTP 镜像启用 fast-exit。

### 2.4 P0：openat 不 follow 符号链接

Linux `openat` 默认 follow symlink。当前 VFS `open_file` 对 `VfsNodeType::Symlink` 返回 `VfsError::NotAFile` → `-EISDIR`。

影响：glibc/musl 通过 symlink 访问文件、脚本、`/proc` 类路径广泛失败。

**收敛建议**：短期对 symlink 路径 `warn!` + 保持 `-EISDIR` 并文档化；中期在 `open_path` 增加 follow（与 `faccessat` 的 `resolve_final_symlink` 复用）。

### 2.5 P1：openat 忽略大量 open flags

`linux_open_flags_to_vfs` 未处理：

| Flag | Linux 语义 | 当前行为 |
|------|-----------|----------|
| `O_EXCL` | 与 `O_CREAT` 联用，存在则 `EEXIST` | 忽略，可能覆盖 |
| `O_NOFOLLOW` | 末段 symlink 不跟随 | 无 follow  anyway，语义混乱 |
| `O_NONBLOCK` | 非阻塞 open | 忽略（pipe/socket 走 `fcntl`） |
| `O_SYNC`/`O_DSYNC` | 同步写 | 忽略 |
| `O_NOATIME` 等 | 不更新 atime | 忽略（atime 未实现） |

**收敛建议**：对 `O_EXCL|O_CREAT`、`O_NOFOLLOW` 在 `sys_openat` 入口检测并 `warn!` + `-EINVAL`。

---

## 3. 逐 syscall 审计

### 3.1 openat (56)

| 项 | 内容 |
|----|------|
| 入口 | `KernelSyscallDispatcher::dispatch_openat` → `sys_openat` |
| 实现 | `sys/openat.rs`；下游 `vfs::active_impl::backend().open`、`vfs::fd::alloc_fd` |
| Linux 语义 | 相对 `dirfd` 打开路径；flags/mode 控制创建、截断、目录 open、`O_CLOEXEC`、`O_PATH`、`O_TMPFILE` 等 |
| 覆盖范围 | **部分**：普通文件/目录/字符设备(`/dev/*`)/proc 路径；`O_CLOEXEC`、`O_PATH`、`O_DIRECTORY`、`O_TMPFILE`(模拟)、`O_CREAT/O_TRUNC/O_APPEND` |
| 未覆盖 | symlink follow、`O_EXCL`、`O_NOFOLLOW`、`O_NONBLOCK`、FIFO/socket 专用 open、块设备直接 open |
| 可靠性 | 路径上限 256B；`mode` 未用于创建权限；`O_TMPFILE` 创建可见 `.wateros-tmpfile-{tid}-{id}` 文件（非 Linux 匿名 inode） |
| 问题 | **P0** 锁序卡死（§2.1）；**P0** wait fast-exit（§2.3）；**P0** 不 follow symlink（§2.4）；**P1** flags 忽略（§2.5）；`O_DIRECTORY` 打开非目录返回 `-EISDIR` 与 Linux `-ENOTDIR` 可能不一致 |
| 收敛 | 未知 flags：`warn!("[syscall] openat(56) dirfd={} flags={:#x} unsupported", dirfd, flags)` → `-EINVAL` |

### 3.2 faccessat (48) / faccessat2 (439)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_faccessat` / `dispatch_unknown`(439) |
| 实现 | `sys/faccessat.rs` |
| Linux 语义 | 检查路径 F_OK/R_OK/W_OK/X_OK；`faccessat2` 支持 `AT_SYMLINK_NOFOLLOW`、`AT_EACCESS`、`AT_EMPTY_PATH` |
| 覆盖范围 | **部分**：路径存在性、简化 permission 位、root 特例、`AT_EMPTY_PATH`、symlink follow（手动 40 层） |
| 未覆盖 | 真实 uid/gid 与 inode owner 匹配（`VfsMetadata` 无 owner）；`AT_SYMLINK_NOFOLLOW` 仍 follow |
| 可靠性 | `faccessat` 正确忽略第 4 参数；无效 mode → `-EINVAL` |
| 问题 | **P1** 权限模型过度宽松（任一 class 位满足即可）；**P1** `AT_SYMLINK_NOFOLLOW` 未生效；W_OK 走 `assert_path_writable` 与 Linux 不完全一致 |
| 收敛 | `AT_SYMLINK_NOFOLLOW` 时若末段为 symlink：`warn!` → `-ELOOP` 或实现 nofollow 元数据路径 |

### 3.3 fchmodat (53)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_fchmodat` |
| 实现 | `sys/fchmodat.rs` → `vfs::chmod_absolute` → `chmod_path` |
| Linux 语义 | 修改路径 mode（`mode & 07777`）；支持 `AT_SYMLINK_NOFOLLOW`（flags 在 fchmodat2，此处无） |
| 覆盖范围 | **部分**：ext4 RW 卷路径；tmpfs 取决于 FS impl |
| 未覆盖 | 字符设备/procfs → `EPERM`；只读挂载 → `EROFS`；symlink 权限（follow 后改目标） |
| 问题 | **P1** 无 flags 参数校验；**P2** 错误码 `Unsupported`→`EPERM` 与 Linux `EROFS` 可能不符 |
| 收敛 | proc/char dev 路径已有 `EPERM`；文档化即可 |

### 3.4 fchownat (54)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_fchownat` |
| 实现 | `sys/fchownat.rs` → `vfs::chown_absolute` |
| Linux 语义 | 修改 uid/gid；`(uid_t)-1`/`(gid_t)-1` 省略；flags：`AT_SYMLINK_NOFOLLOW`、`AT_EMPTY_PATH` |
| 覆盖范围 | **部分**：ext4 路径 chown；`AT_EMPTY_PATH` 显式 `-EINVAL` |
| 未覆盖 | `AT_SYMLINK_NOFOLLOW`；capability/CAP_CHOWN 检查 |
| 问题 | **P1** `AT_SYMLINK_NOFOLLOW` 忽略；**P1** 无特权校验（bring-up 可接受） |
| 收敛 | 非 ext4/tmpfs 卷：`warn!` → `-EPERM` |

### 3.5 readlinkat (78)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_readlinkat` |
| 实现 | `sys/readlinkat.rs` |
| Linux 语义 | 读 symlink 内容；不自动 NUL 终止缓冲区（但可写 NUL 若空间足够） |
| 覆盖范围 | **部分**：真实 symlink；`/proc/self/exe`、`/proc/thread-self/exe` 特例 |
| 未覆盖 | procfs 其他 symlink；缓冲区无 NUL 时行为与 Linux 一致 |
| 问题 | **P2** `/proc/self/exe` 在 `bufsiz > len` 时写 NUL（Linux 不写）；**P2** 非 symlink → `-EINVAL`（Linux 亦如此） |
| 收敛 | 保持；文档注明 proc 特例 |

### 3.6 statfs (43)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_statfs` |
| 实现 | `sys/statfs.rs` |
| Linux 语义 | 返回文件系统统计；`f_type` 应对应 magic |
| 覆盖范围 | **stub**：路径存在性检查后填**硬编码**常量（块数/文件数等） |
| 问题 | **P1** 数据虚假，不影响功能但误导配额/磁盘检测；magic 来自 `mount_statfs_magic` 或默认 ext4 `0xEF53` |
| 收敛 | `warn!("[syscall] statfs(43) path={:?} using stub counters", path)`；或对接真实 `statvfs` |

### 3.7 fstat (80) / fstatat (79) / statx (291)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_fstat` / `dispatch_unknown`(79/291) |
| 实现 | `sys/fstat.rs` + `linux_stat.rs` + `stat_times.rs` |
| Linux 语义 | 取元数据；`fstatat`/`statx` 支持 `AT_EMPTY_PATH`、`AT_SYMLINK_NOFOLLOW`；`statx` 按 mask 填充 |
| 覆盖范围 | **部分**：普通 fd/路径；`AT_EMPTY_PATH`；`statx` 固定 `STATX_SUPPORTED` mask |
| 未覆盖 | `AT_SYMLINK_NOFOLLOW`；`statx` 按请求 mask 裁剪；`st_uid/st_gid`；真实 atime/ctime/btime |
| 问题 | **P1** owner 恒 0；**P1** 时间戳除 `utimensat` 外为 0；**P1** `fstatat` flags 未校验非法位；**P2** 目录 fd `fstat` 返回目录 meta（OK） |
| 收敛 | 非法 flags：`warn!` → `-EINVAL`；未实现 mask 清零并 `warn!` |

### 3.8 lseek (62)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_lseek` |
| 实现 | `sys/lseek.rs` |
| Linux 语义 | SEEK_SET/CUR/END；pipe/socket → `ESPIPE` |
| 覆盖范围 | **已实现**：三种 whence；`EINVAL`→`ESPIPE` 映射 |
| 问题 | **P2** 返回值截断为 `usize`（大文件偏移 >4G 在 LP32 才有影响，riscv64 OK） |
| 收敛 | 无需 |

### 3.9 getdents64 (61)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_getdents64` |
| 实现 | `sys/getdents64.rs` → `DirectoryHandle::fill_getdents64` |
| Linux 语义 | 目录 fd 枚举；`d_off`/`d_reclen`/`d_type` |
| 覆盖范围 | **部分**：目录 fd；首次调用加载全目录缓存 |
| 未覆盖 | `DT_UNKNOWN`；telldir/seekdir 与 cookie 一致性 |
| 问题 | **P2** 非目录 fd → `-ENOTDIR`；大目录首次 load 可能长时间持锁 |
| 收敛 | 非目录：`ENOTDIR` 已正确 |

### 3.10 mkdirat (34)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_mkdirat` |
| 实现 | `sys/mkdirat.rs` → `vfs::mkdir_absolute` |
| Linux 语义 | 创建目录；`mode` 受 umask 影响 |
| 覆盖范围 | **部分**：路径解析 + mkdir；**未**应用 umask |
| 问题 | **P0** wait fast-exit（§2.3）；**P1** mode 未掩码；只读卷 `EROFS` |
| 收敛 | 同 openat fast-exit 策略 |

### 3.11 symlinkat (36)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_symlinkat` |
| 实现 | `sys/symlinkat.rs` → `vfs::symlink_absolute` |
| Linux 语义 | 创建 symlink；`target` 可为相对串 |
| 覆盖范围 | **部分**：RW 卷；`PATH_MAX` 4096 |
| 问题 | **P2** 只读/proc `EROFS`；已存在 `EEXIST` |
| 收敛 | 无需 |

### 3.12 unlinkat (35)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_unlinkat` |
| 实现 | `sys/unlinkat.rs` → `vfs::unlink_absolute` |
| Linux 语义 | 删文件；`AT_REMOVEDIR` 删空目录 |
| 覆盖范围 | **部分**：同卷 RW；非空目录错误由 FS 返回 |
| 问题 | **P0** wait fast-exit；**P2** 删目录未设 `AT_REMOVEDIR` → `-EISDIR` |
| 收敛 | 同 §2.3 |

### 3.13 renameat2 (276)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_renameat2` |
| 实现 | `sys/renameat2.rs` → `vfs::rename_absolute` |
| Linux 语义 | 重命名；flags：`RENAME_EXCHANGE`、`RENAME_NOREPLACE`、`RENAME_WHITEOUT` 等 |
| 覆盖范围 | **最小**：`flags==0` 同 RW 卷 rename；跨卷 `-EINVAL`（`Unsupported`） |
| 问题 | **P1** 所有非零 flags → `-EINVAL`（应分项支持或明确拒绝并 warn）；**P1** 非原子 journal 语义（注释已说明） |
| 收敛 | `flags != 0`：`warn!("[syscall] renameat2(276) flags={:#x} unsupported", flags)` → `-EINVAL` |

### 3.14 utimensat (88)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_utimensat` |
| 实现 | `sys/utimensat.rs` + `stat_times.rs` |
| Linux 语义 | 设置 atime/mtime；`UTIME_NOW`/`UTIME_OMIT`；`AT_SYMLINK_NOFOLLOW` |
| 覆盖范围 | **部分**：内存旁路表；`times==NULL` → now；`path==NULL` + dirfd fd 语义 |
| 未覆盖 | 持久化到 ext4 inode；`AT_SYMLINK_NOFOLLOW`；ctime 更新 |
| 问题 | **P1** 重启丢失；**P1** symlink flags 忽略；**P2** 仅校验 `AT_SYMLINK_NOFOLLOW` 一位 |
| 收敛 | 文档化「非持久」；长期写入 ext4 xattr 或 inode |

### 3.15 sync (81) / fsync (82) / fdatasync (83)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_sync` / `dispatch_fsync` / `dispatch_fdatasync` |
| 实现 | `sys/sync.rs` |
| Linux 语义 | `sync` 触发全局写回；`fsync`/`fdatasync` 单 fd（后者不刷元数据） |
| 覆盖范围 | **部分**：`handle.flush()`；`sync` 调 `flush_all_open_files` 恒返回 0 |
| 未覆盖 | `fdatasync` 与 `fsync` 区分（当前相同路径） |
| 问题 | **P0** 页缓存 flush 锁序（§2.1）；**P1** `fdatasync` 未省略元数据刷写 |
| 收敛 | pipe/socket/不支持 seek 的 fd：已有 `ESPIPE`/`EINVAL` 映射 |

### 3.16 ftruncate (46)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_ftruncate` |
| 实现 | `sys/ftruncate.rs` |
| Linux 语义 | 截断/扩展已打开文件 |
| 覆盖范围 | **部分**：`PagedFileHandle`/`BufferedFileHandle` |
| 问题 | **P2** 目录/pipe → `-EINVAL`；只读 fd → `EBADF`/`EINVAL` |
| 收敛 | 无需 |

### 3.17 fallocate (47)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_fallocate` |
| 实现 | `sys/fallocate.rs` |
| Linux 语义 | 预分配；`FALLOC_FL_KEEP_SIZE`/`PUNCH_HOLE` 等 |
| 覆盖范围 | **最小**：无 KEEP_SIZE 时 extend truncate；`PUNCH_HOLE` → `-EOPNOTSUPP` |
| 问题 | **P1** `KEEP_SIZE` 且需扩展 → `-EOPNOTSUPP`；**P2** `len==0` 成功 |
| 收敛 | 已拒绝 `PUNCH_HOLE`；其他 mode `warn!` → `-EOPNOTSUPP` |

### 3.18 mount (40)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_mount` |
| 实现 | `sys/mount.rs` + `vfs::mount_*` + `mount_table.rs` |
| Linux 语义 | 挂载 fs；`MS_RDONLY`、`MS_REMOUNT`、bind、propagation 等 |
| 覆盖范围 | **部分**：ext2/3/4/vfat 块设备、tmpfs、proc、cgroup/cgroup2、`MS_REMOUNT|MS_RDONLY` |
| 未覆盖 | bind mount、`MS_SHARED`、`MS_MOVE`、nfs/fuse/overlay、flags 高位 |
| 问题 | **P0** 同步重 I/O 假死（§2.2）；**P0** wait fast-exit（§2.3）；**P1** 非支持 fstype `-EINVAL`；**P1** `MS_REMOUNT` 仅支持加 `MS_RDONLY`；错误映射 `Driver/NotFound`→`ENOENT` |
| 收敛 | 见 §2.2；`flags & !(MS_RDONLY|MS_REMOUNT)` 且非 0 时显式 `warn!`+`-EINVAL` |

### 3.19 umount2 (39)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_umount2` |
| 实现 | `sys/umount2.rs` → `vfs::unmount_at` |
| Linux 语义 | 卸载；`MNT_FORCE`、`MNT_DETACH`、`UMOUNT_NOFOLLOW` 等 |
| 覆盖范围 | **最小**：`flags` **完全忽略**；从 `AUX_MOUNTS` 移除 |
| 未覆盖 | 繁忙检测、`MNT_DETACH` lazy umount、引用计数 |
| 问题 | **P1** 有打开 fd 仍卸载成功 → 后续 I/O 异常；**P1** flags 忽略 |
| 收敛 | `flags != 0`：`warn!("[syscall] umount2(39) flags={:#x} unsupported", flags)` → `-EINVAL` |

### 3.20 getcwd (17)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_getcwd` |
| 实现 | `sys/getcwd.rs` → `vfs::cwd::write_cwd_to_buf` |
| Linux 语义 | 写 NUL 结尾 cwd；buf 不足 `ERANGE`；成功返回 buf 指针 |
| 覆盖范围 | **部分**：内核缓冲 **固定 256 字节** |
| 问题 | **P1** 深路径 >255 → `-ERANGE`（Linux `PATH_MAX` 4096）；**P2** `size==0` → `-EINVAL` |
| 收敛 | 增大缓冲至 4096 或动态分配；超长 `warn!` |

### 3.21 chdir (49)

| 项 | 内容 |
|----|------|
| 入口 | `dispatch_chdir` |
| 实现 | `sys/chdir.rs` → `vfs::cwd::chdir_current` |
| Linux 语义 | 切换 per-task cwd；须为目录 |
| 覆盖范围 | **已实现**：路径解析 + 目录校验 |
| 问题 | **P2** 中间 symlink 组件 follow 取决于 `chdir_current` 实现；权限未检查 |
| 收敛 | 无需短期 |

---

## 4. 优先级汇总

### P0（卡死 / wait 交互 / 严重语义）

| ID | 项 | syscall | 建议 |
|----|----|---------|------|
| VFS-P0-01 | 页缓存 flush 与 ext4 锁序死锁 | fsync/fdatasync/sync, openat | 遵守 `paged_handle` 锁序；危险组合 warn |
| VFS-P0-02 | mount 块设备同步重 I/O | mount(40) | 显式拒绝未支持 flags；文档化超时风险 |
| VFS-P0-03 | LTP wait fast-exit 篡改 open/mount/mkdir/unlink 语义 | openat, mount, mkdirat, unlinkat | feature 门控或默认禁用 |
| VFS-P0-04 | openat 不 follow symlink | openat(56) | 实现 follow 或明确 warn+EISDIR |

### P1（语义偏差 / 误导性成功）

| ID | 项 | syscall |
|----|----|---------|
| VFS-P1-01 | open flags O_EXCL/O_NOFOLLOW 等忽略 | openat |
| VFS-P1-02 | stat uid/gid/时间戳不完整 | fstat/fstatat/statx |
| VFS-P1-03 | statfs 硬编码假数据 | statfs |
| VFS-P1-04 | faccessat 权限模型简化 | faccessat(2) |
| VFS-P1-05 | renameat2 拒绝所有 flags | renameat2 |
| VFS-P1-06 | umount2 忽略 flags / 无繁忙检测 | umount2 |
| VFS-P1-07 | utimensat 非持久 | utimensat |
| VFS-P1-08 | getcwd 256 字节上限 | getcwd |
| VFS-P1-09 | fdatasync 与 fsync 未区分 | fdatasync |
| VFS-P1-10 | fallocate KEEP_SIZE 扩展失败 | fallocate |

### P2（次要 / 已知 bring-up 限制）

- `O_TMPFILE` 非匿名 inode
- `readlinkat` proc NUL 终止差异
- `chown`/`chmod` 无 capability 检查
- `rename` 非 journal 原子

---

## 5. 统一 warn 模板

```rust
log::warn!(
    "[syscall] {}(nr={}) arg0={:#x} arg1={:#x} arg2={:#x} arg3={:#x} — {}",
    name, nr, args.arg(0), args.arg(1), args.arg(2), args.arg(3), reason
);
```

错误码约定（与 `vfs_util::vfs_error_to_errno` 对齐）：

| 场景 | errno |
|------|-------|
| 未实现 flag/option | `-EINVAL` 或 `-EOPNOTSUPP` |
| 未实现 syscall 变体 | `-ENOSYS` |
| 只读文件系统 | `-EROFS` |
| 权限（bring-up） | `-EPERM` |
| 路径/设备不存在 | `-ENOENT` |

---

## 6. 测试建议

1. **锁序**：并发 `openat` + `read` + `fsync` 压力（多文件、页缓存 miss）
2. **mount**：无效块设备、重复挂载、`MS_REMOUNT`、卸载后 fd 仍 open
3. **wait**：禁用 fast-exit 后跑 cgroup regression，确认不再误 `exit(0)`
4. **symlink**：`openat`/`faccessat`/`readlinkat` 组合
5. **边界**：`getcwd` 长路径、`renameat2` 跨目录、`O_EXCL|O_CREAT`
