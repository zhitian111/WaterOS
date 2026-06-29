# wateros-vfs — 已实现功能快照

## 用途

记录 `wateros-vfs` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-vfs/**` 源码与 `Cargo.toml`；根 `wateros` 通过 `vfs-bridge` feature 启用聚合 crate。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-vfs`（聚合） | `api` 契约 re-export、`active_impl` 选后端、`root`/`mount`/`fd`/`cwd` 对外面 | 已实现 |
| `wateros-vfs-api-v0` | `VfsBackend`、路径解析、fd 会话、挂载表 trait | 已实现 |
| `vfs-impl/impl-dummy` | 占位 `VfsBackend`（无真实 I/O） | 已实现（测试/无 bridge） |
| `vfs-impl/impl-fs-bridge` | 桥接 `wateros-fs` 根卷、devfs、procfs、辅助挂载 | 已实现（默认） |
| `vfs-impl/impl-fd-session` | per-task fd 表、cwd、pipe、字符设备句柄 | 已实现（默认） |
| `vfs-impl/impl-page-cache` | 文件页缓存（写回、逻辑 size 覆盖） | 已实现 |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `api-v0` | 链接 API 契约与 dummy 桩 |
| `bridge-fs-api` | 启用 `impl-fs-bridge` + 依赖 `wateros-fs` |
| `impl-fd-session` | per-task fd/cwd，依赖 `wateros-task` |
| `impl-dummy` | 仅占位后端（与 `bridge-fs-api` 互斥使用场景） |
| `default` | `api-v0` + `bridge-fs-api` + `impl-fd-session` |

## 已实现能力

- **路径**：绝对路径规范化、`resolve_open_path`、cwd 相对解析、根文件名校验。
- **单根只读**：`vfs::root::read_view()` → `exists` / `metadata` / `read`。
- **RW 会话**：`mount::open_rw_session` 按 `VfsFsKind` 打开根卷写会话。
- **open/read/write/seek**：`VfsOpenOps` + `VfsIoHandle`（经 bridge 到 ext4 与页缓存）。
- **挂载命名空间**：ext4 块设备、tmpfs、cgroup、procfs、securityfs、bind/move、传播类型、lazy umount。
- **路径级元数据操作**：`chmod`/`chown`/`xattr`/`truncate`/`mkdir`/`unlink`/`rename`/`symlink`/`mknod` socket。
- **fd 会话**：stdin/stdout/stderr、动态 fd 分配、pipe/stream pair、文件锁（`flock`）、控制台与 `/dev/*` 字符设备。
- **per-task cwd**：spawn 继承、chdir、`lookup_argv`/`lookup_exe` 供 procfs。
- **自检**：`vfs::test()` 串联 api、dummy、bridge、fd、cwd 与 `self_test::run()`。

## 与 wateros-fs 的分工

`wateros-vfs` **不**直接实现块文件系统；`bridge-fs-api` 下由 `impl-fs-bridge` 调用 `wateros-fs` 的 `rootfs`/`devfs`/`procfs` 与 ext4 impl。VFS 层负责路径路由、挂载表、页缓存叠加与 syscall 侧 fd 语义。

## 缺口与后续

- `impl-dummy` 无真实挂载与 open，仅编译占位。
- 页缓存与 ext4 写路径的并发策略仍偏单核 bring-up 假设。
- `VfsFdSession::alloc_fd` 默认 trait 方法返回 `Unsupported`，真实分配在 `impl-fd-session`。
- 异步 I/O（`FsAsyncIo` 对应面）未在 VFS 暴露。
- securityfs/cgroup 为伪 FS 子集，语义以满足 LTP 子集为主。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |
