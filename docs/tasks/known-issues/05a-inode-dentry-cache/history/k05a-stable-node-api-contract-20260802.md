# K-05A 稳定节点 API 契约阶段报告

```text
task: K-05A stable node API contract
date: 2026-08-02
base_commit: aaa3864f
scope: fs-api/api-v0 only; backend and VFS adoption pending
task_architecture_changed: no
```

## 结论

FS API 已具备用后端无关节点身份承载 open-file I/O 的最小契约。新增
`FsNodeId`，并在 `ReadWriteFs` 中增加 open、close、metadata、range read/write 和
truncate 的稳定节点方法。默认实现返回 `Unsupported`，未强制其它文件系统伪造支持。

## 契约不变量

- `FsNodeId` 只在创建它的文件系统实例和 mount generation 内有效。
- `open_node()` 成功后必须恰好调用一次 `close_node()`。
- rename 后 identity 继续指向原节点；unlink 后不能重定向到复用该 inode 的新文件。
- 数值只允许用于缓存键和诊断，VFS 不得自行解释后端 inode 布局。
- `LocalRwFs` 完整转发所有稳定节点方法，保持 aggregate/impl 分层。

## 涉及文件

- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`

## 验证

- `cargo test --manifest-path os/components/wateros-fs/fs-api/api-v0/Cargo.toml --offline`：通过。
- `cd os && make rv_check`：通过。
- `cd os && make la_check`：通过。
- `git diff --check`：通过。

## 后续阶段

该提交只冻结接口，不改变运行时路径。后续提交依次完成：

1. another_ext4 按 inode 实现稳定 I/O、open 引用和 unlink/rename-overwrite 保留。
2. VFS 以 `(mount identity, FsNodeId)` 作为页缓存身份，并用共享 lease 配对 close。
3. 删除 16 MiB detached 文件堆拷贝上限，完成 hardlink/rename/unlink-open-fd 回归。

K-05A 总任务保持开放，完整 BuildStorm、FS LTP 和 `e2fsck -fn` 尚未由本阶段覆盖。
