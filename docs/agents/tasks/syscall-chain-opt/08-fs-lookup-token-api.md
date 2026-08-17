# 任务 08：建立可复用的 FS lookup token 契约

## 任务内容与目标

在不改变路径语义的前提下，让一次后端 lookup 同时产出 inode/node identity 与 metadata，
并允许 symlink read、stable open 复用该结果。为下一任务消除 `lookup + getattr` 重复打基础；
本提交只引入契约和后端实现，不切换 syscall walker。

## 实施方案

1. 在 `fs-api/api-v0` 增加窄的 lookup 结果类型，至少携带 `FsNodeId`、`FsMetadata` 和
   “是否可稳定持有”信息；写清 rename/unlink 后的有效期。
2. 提供默认回退实现，避免强迫 ramfs/伪 FS 一次完成；another-ext4 覆盖为一次 lookup +
   getattr，并提供按 token readlink/open 的优化入口。
3. stable open 成功后必须持有后端 open ref；普通 walker token 不得延长 inode 生命周期。
4. API 不能把 another-ext4 私有 inode 类型泄漏到 VFS。
5. 同步 API rustdoc 和受影响组件文档。

## 涉及文件

- `os/components/wateros-fs/fs-api/api-v0/src/{traits,types或新lookup模块}.rs`
- `os/components/wateros-fs/fs-api/api-v0/src/handles.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/{operations,filesystem}.rs`
- ramfs/其它实现的默认适配与测试

## CodeGraph 查询

```bash
codegraph explore "ReadOnlyFs metadata read_symlink ReadWriteFs open_node"
codegraph impact "ReadOnlyFs"
codegraph callers "open_node"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-fs/fs-api/api-v0/Cargo.toml
cargo test --offline --manifest-path components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
make rv_check && make la_check
cd .. && git diff --check
```

旧调用者行为不变；定向测试证明 another-ext4 lookup token 只执行一次路径 lookup，token 不会
在 unlink/inode reuse 后错误指向新文件。

## Commit 与简报

提交建议：`[refactor] 增加可复用 FS lookup token`。新增 `history/08-brief.md`，明确 API
影响与所有实现同步情况。
