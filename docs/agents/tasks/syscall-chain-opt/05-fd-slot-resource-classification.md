# 任务 05：FD slot 分类与单次 I/O 快照

## 任务内容与目标

为 read/pread 快路径建立唯一事实源：FD slot 同时保存 `SharedIoHandle`、slot flags、
资源分类和终端标识。一次 registry 查询返回 `FdIoLease`，普通文件不再反复执行
socket/TTY/Unix/epoll 负向探测或重复取得全局 FD registry。

## 实施方案

1. 在 VFS API 定义窄的 `VfsResourceKind`，覆盖 regular/directory/pipe/socket/terminal/
   unix/epoll/other；不得让 VFS 依赖 syscall 私有实现类型。
2. `VfsIoHandle` 提供基于稳定句柄能力的保守默认分类，各主要具体句柄显式覆盖；安装 slot
   时缓存分类。
3. 将 path-only/cloexec 等 slot flags 与 slot 放在同一表项，避免平行 Vec 漂移。
4. dup、fork copy、`CLONE_FILES` share、close_range、exec cloexec 全量保持分类与 flags。
5. VFS facade 提供一次性 `FdIoLease`；`read`、`readv`、`pread64` 的权限、`O_PATH`、TTY、
   socket 与实际 I/O 均消费同一 lease，不在 syscall 内重新查询 FD registry。
6. 增加资源分类安装、dup、替换、share/copy 的数据模型测试。

## 涉及文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/` 的 handle/FD 类型
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- 各 `VfsIoHandle` 实现所在 pipe/socket/TTY/epoll/Unix 模块
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`

## CodeGraph 查询

```bash
codegraph explore "PerTaskFdRegistry SharedIoHandle VfsIoHandle alloc_fd_for_task"
codegraph impact "VfsIoHandle"
codegraph explore "sys_read sys_readv sys_pread64 acquire_read_lease fd registry"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-vfs/vfs-impl/impl-fd-session/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
cd .. && git diff --check
```

现有 read/close 行为不变；新增测试证明分类不会在 dup/fork/close_range 后丢失。静态审查
需确认 `read`、`readv` 与 `pread64` 各只调用一次 `current_io_lease`，后续不再调用
`socket_fd::lookup`、`is_path_only_fd`、`current_fd_is_tty_char` 或 `with_current_io`。
本任务不以墙钟收益为合入条件，但必须无 BuildStorm 功能回退。

## Commit 与简报

提交建议：`[refactor] FD slot 缓存资源分类与 flags`。新增 `history/05-brief.md`。
