# 任务 05：在 FD slot 安装稳定资源分类

## 任务内容与目标

为后续 read/close 快路径建立唯一事实源：FD slot 同时保存 `SharedIoHandle`、slot flags 和
资源分类，普通文件不再依赖 socket/TTY/Unix/epoll 侧表的负向探测。本提交只改数据模型和
生命周期接线，不重写 syscall 控制流。

## 实施方案

1. 在 VFS API 定义窄的 `VfsResourceKind`，覆盖 regular/directory/pipe/socket/terminal/
   unix/epoll/other；不得让 VFS 依赖 syscall 私有实现类型。
2. `VfsIoHandle` 提供默认 `Other` 分类，各具体句柄覆盖；安装 slot 时缓存分类。
3. 将 path-only/cloexec 等 slot flags 与 slot 放在同一表项，避免平行 Vec 漂移。
4. dup、fork copy、`CLONE_FILES` share、close_range、exec cloexec 全量保持分类与 flags。
5. 增加所有资源类型安装、dup、替换、share/copy 的数据模型测试。

## 涉及文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/` 的 handle/FD 类型
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- 各 `VfsIoHandle` 实现所在 pipe/socket/TTY/epoll/Unix 模块

## CodeGraph 查询

```bash
codegraph explore "PerTaskFdRegistry SharedIoHandle VfsIoHandle alloc_fd_for_task"
codegraph impact "VfsIoHandle"
codegraph callers "io_handle_for_task"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-vfs/vfs-impl/impl-fd-session/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
cd .. && git diff --check
```

现有 read/close 行为不变；新增测试证明分类不会在 dup/fork/close_range 后丢失。本任务不以
墙钟收益为合入条件，但必须无 BuildStorm 功能回退。

## Commit 与简报

提交建议：`[refactor] FD slot 缓存资源分类与 flags`。新增 `history/05-brief.md`。
