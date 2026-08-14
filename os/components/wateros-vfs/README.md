# wateros-vfs

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-vfs` 是 WaterOS 的虚拟文件系统聚合 crate。它通过 [`api`] 定义基本能力契约，
`active_impl` 选择后端，并把能力组合为 `root`、`mount`、`self_test` 等对外稳定接口。它把
`wateros-fs` 的根卷/devfs 暴露成统一的 fd 会话与文件句柄，供 syscall 层使用。

## 模块分层


| 层       | 路径                        | 职责                                                                                              |
| ---------- | ----------------------------- | --------------------------------------------------------------------------------------------------- |
| 聚合门面 | `src/lib.rs`                | re-export`api`、`fd`、`cwd`、`mount_ns`；路径解析、符号链接、user-graphics 入口。                 |
| VFS API  | `vfs-api/api-v0/`           | `VfsBackend`、`VfsIoHandle`、`VfsFdSession`、`VfsMountOps`、设备映射等契约；不依赖 `wateros-fs`。 |
| fd 会话  | `vfs-impl/impl-fd-session/` | per-task fd 表、cwd、文件锁，以及控制台/pipe/char dev 等`VfsIoHandle`。                           |
| FS 桥接  | `vfs-impl/impl-fs-bridge/`  | 把`VfsBackend` 桥接到 `wateros-fs`：目录/文件/paged/proc/tmpfs 句柄与挂载表。                     |
| 页缓存   | `vfs-impl/impl-page-cache/` | 全局共享文件页缓存（Direct 模式，LRU）。                                                          |

## 实现说明

- `api` 只定义能力契约（`VfsBackend` / `VfsIoHandle` / `VfsFileHandle` / `VfsFdSession` /
  `VfsMountOps` 等），不依赖 `wateros-fs`；具体行为由 `impl-fs-bridge` 等实现。
- 三个主要 feature 面：
  - `bridge-fs-api`：启用 `impl-fs-bridge`，把 VFS 桥接到 `wateros-fs`（根卷、devfs、procfs、
    tmpfs、挂载表）。
  - `impl-fd-session`：启用 per-task fd 表、cwd、文件锁与各类 `VfsIoHandle`（依赖 base/task/
    arch/debug/tty）。
  - `user-graphics`：在 fd-session 上启用 `/dev/fb0` 与 evdev（`initialize_user_graphics_devices`、
    `user_graphics_input_worker`）。
- 读路径采用预约模型：`VfsReadLease` / `VfsPreparedRead` / `VfsReadFinish`，用户复制成功后
  提交、失败回滚；页缓存与 fd 会话都复用该语义。
- 设备映射：`VfsDeviceMapping` / `VfsDeviceMappingLease` / `VfsFramebufferInfo`，供
  `/dev/fb0` 的 mmap 与 lease 生命周期管理。
- 路径解析：`normalize_absolute_path`、`resolve_against_cwd`、`resolve_open_path`、
  `resolve_symlink_path_with`（`FinalSymlink`）；符号链接展开与挂载路由在桥接层完成。
- 页缓存 Lock ordering：`files` → per-file `FileEntryInner` → `state` → 根卷 `SharedRwFs`，
  禁止逆序；`state` 锁内不得调用下层块设备 I/O。

## 调用链路

打开路径：

```text
sys_openat
  -> resolve_open_path（cwd + 符号链接展开）
  -> VfsOpenOps / 后端（FsBridge → wateros-fs 或特殊设备 open_special_device）
  -> 得到 Box<dyn VfsIoHandle>，登记到 PerTaskFdRegistry
```

读取路径：

```text
read(2)
  -> PerTaskFdRegistry 定位 fd
  -> VfsIoHandle / VfsFileHandle（fs-bridge 句柄）
  -> VfsReadLease 预约 -> 用户复制 -> finish 提交/回滚
  -> 页缓存（impl-page-cache）-> ext4（wateros-fs）
```

挂载 / user-graphics：

```text
mount
  -> VfsMountOps / mount_table（resolve_route / FsRoute）
Nano-X 打开 /dev/fb0
  -> open_special_device -> FramebufferHandle / EvdevHandle（user_graphics）
```

## 各实现功能

### vfs-api / VFS 公共 API

主要实现在 `vfs-api/api-v0/src/`。

- 定义统一的打开态句柄契约：`VfsIoHandle` / `VfsFileHandle` 把读、写、seek、close、poll 等
  语义抽象成 trait，syscall 层只依赖这些抽象，不感知具体文件系统后端或设备类型。
- 提供预约式读取：先 `prepare_read` 预约输入字节，再以 `VfsReadLease` 持有 staged data，最后
  `finish` 提交或回滚；用户复制失败时返回 `Fault`，未消费字节不会丢失，避免并发读取重复消费。
- 提供文件内容版本化：`VfsFileContentIdentity` 在内容变更后递增版本号，缓存消费者把版本纳入
  键，跨 close/reopen 保持稳定，避免读到旧内容缓存。
- 提供 framebuffer 的中立视图与区域校验：`VfsFramebufferInfo` / `VfsFramebufferRegion` 让
  syscall 层不依赖具体 VirtIO 驱动即可实现 `/dev/fb0` 的 mmap 与区域刷新，`fits` 用
  checked 运算拒绝越界与溢出。
- 提供路径解析与规范化：`normalize_absolute_path` 规整 `..`/`.`（如 `/a/./b/../c` →
  `/a/c`），`resolve_open_path` 结合 cwd 解析，`resolve_symlink_path_with` 按 `FinalSymlink`
  决定是否跟随最终符号链接。
- 定义 fd 会话、挂载与节点视图契约：固定 stdin/stdout/stderr fd（`VFS_STDIN_FD` 等）与动态
  fd 起点、`VfsMountOps` / `VfsMountTable`、devfs 节点视图与 `VfsError` 统一错误分类。

### impl-fd-session / per-task fd 会话

主要实现在 `vfs-impl/impl-fd-session/src/`。

- 提供 per-task fd 表：以 `TaskId` 为 key 维护每个任务的打开文件集合，`close` 幂等（`closed`
  标志防止重复关闭），并区分普通 fd 与 `FD_CLOEXEC`（exec 时自动关闭）、`FD_PATH_ONLY`
  （`O_PATH`，只参与路径解析，不可读写）。
- 提供控制台与 pipe 句柄：`ConsoleInHandle` / `ConsoleOutHandle` 桥接 tty；匿名 pipe、命名
  pipe 与 Unix 流 socket 对（`pipe_handle_pair` / `stream_pair_handle_pair`）成对创建，读端/
  写端/对端生命周期独立管理。
- 提供各类设备句柄：null、zero、urandom、CPU-dma-latency 等特殊设备，以及 RTC 等字符设备
  句柄（含 devfs 路径元数据查询）。
- 提供 per-task cwd、进程 root 与文件锁：`PerTaskCwdRegistry` 维护工作目录及 `chroot(2)`
  根边界，fork 复制、`CLONE_FS` 共享，并约束绝对路径、`..`、符号链接和保存的 dirfd；
  `Flock` 按 inode 索引实现 `LOCK_SH` / `LOCK_EX` / `LOCK_UN` / `LOCK_NB` 语义。
- 提供用户图形特殊设备：`user-graphics` 下打开 `/dev/fb0` 与 evdev，并以低优先级 worker 轮询
  输入、广播给各打开者；未启用时这些入口编译为空实现。

### impl-fs-bridge / 文件系统桥接

主要实现在 `vfs-impl/impl-fs-bridge/src/`。

普通文件句柄以稳定 inode 支撑 unlink 后 I/O；`O_TMPFILE` 直接创建无目录项节点，支持
`linkat(AT_EMPTY_PATH)` 同挂载发布，并在最后一个 fd 关闭时回收未发布节点。

- 把 VFS 桥接到 `wateros-fs`：零大小 `FsBridge` 后端把 `FsError` / `FsKind` 映射为
  `VfsError` / `VfsFsKind`，使 VFS 层不依赖具体 ext4 实现即可访问根卷与 devfs。
- 支持两种根卷文件句柄：小文件以 `BufferedFileHandle` 全文缓冲于内存（`RootFileHandle` 为兼容
  别名），大文件走 `PagedFileHandle` 与页缓存协作，按文件规模选择路径。
- 提供目录、procfs 与 tmpfs：`DirectoryHandle` 目录遍历、procfs 视图句柄、tmpfs 策略层。
- 支持 per-task 挂载命名空间与辅助挂载表：可挂 RW / RO / procfs 伪挂载 / bind 别名，维护
  `MountPropagation`（Private/Shared/Slave/Unbindable），路径经 `resolve_route` 按最长前缀
  路由到对应卷。

### impl-page-cache / 文件页缓存

主要实现在 `vfs-impl/impl-page-cache/src/lib.rs`。

- 提供全局共享文件页缓存：以"挂载代次 + 稳定 (mount_id, node_id)（或路径）"为身份，LRU 缓存
  页帧；多读者可并发，写/刷盘独占，同一页在多个打开者间共享。
- 维护脏页与预取：跟踪每文件脏页索引与上次读到的页号，连续脏页合并为一次批量写回，顺序读时
  触发预取，减少下层块设备 I/O 往返。
- 支持挂载代次切换时原地重置：`reset_to_gen` 清空缓存元数据并复用已分配帧池，避免每次
  mount/umount 重建 16 MiB 缓存造成内核堆碎片化与长跑卡死。
- 保证缺页与写回安全：`install_page` / `install_zero_page` 在调用下层 I/O 前先释放内部锁，
  写回路径按固定锁顺序（files → 文件条目 → state → 根卷）访问，避免锁反转与持锁下探块设备。

无 FS 桥接时，聚合层内部返回 `NotMounted`/`Unsupported`，不再维护独立占位 crate。
