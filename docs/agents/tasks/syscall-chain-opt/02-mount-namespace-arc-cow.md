# 任务 02：将 mount namespace 改为 Arc 快照与 COW

## 任务内容与目标

让路由热路径只在全局 registry 中取得并克隆 `Arc<MountNamespace>`，不再深拷贝 entries
及字符串。共享 namespace 的 mount 变化仍对共享者可见；`CLONE_NEWNS`、unshare 和独立
spawn 使用 copy-on-write 隔离。

## 实施方案

1. `PerTaskMountNsRegistry` 存储 `Arc<MountNamespace>`；route 返回 Arc 快照后立即释放锁。
2. 对真正共享同一 namespace 的任务继续通过 owner 映射共享同一槽位。
3. 独立复制先 Arc clone，首次 mount/umount/remount/propagation 时 `Arc::make_mut`。
4. bootstrap namespace 采用同样快照语义；不得在持 registry 锁时访问 FS 或递归路由。
5. 增加 share/copy/unshare/COW 和 mount 可见性单元测试。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{mount_table,mount_ns}.rs`
- `os/components/wateros-vfs/src/mount_ns.rs`
- 相关 self-test 文件

## CodeGraph 查询

```bash
codegraph explore "MountNamespace mount_namespace_snapshot with_current_namespace"
codegraph impact "MountNamespace"
codegraph callers "copy_mount_ns_from_parent"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-vfs/vfs-impl/impl-fs-bridge/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
cd .. && git diff --check
```

任务 01 的 namespace deep-clone 计数在普通 open/stat 路径降为零；mount namespace 语义测试
全部通过。用任务 00 runner 做 BuildStorm A/B，至少确认无性能回退和 mount/procfs 异常。

## Commit 与简报

提交建议：`[perf] mount namespace 使用 Arc COW 快照`。新增 `history/02-brief.md`。
