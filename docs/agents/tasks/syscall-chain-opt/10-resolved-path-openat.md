# 任务 10：openat 消费稳定 ResolvedPath

## 任务内容与目标

让路径解析结果携带 namespace snapshot、mount identity、规范路径、metadata 和可用的稳定
node lease，供 openat 权限检查、open_node 和 inotify 直接消费。消除最终节点反复 metadata、
exists 和 open lookup，同时正确支持创建不存在的最终节点。

## 实施方案

1. 定义短生命周期 `ResolvedPath`：`Existing`、`Missing { stable_parent, basename }`、
   pseudo/special fallback 等明确变体。
2. Existing 的 metadata 仅供本次 syscall；稳定 lease 保证 rename/unlink 后打开的是解析到的
   inode。Missing 创建必须基于已解析父目录，避免重新走全路径。
3. backend 新增 resolved-open 窄入口；不把解析对象扩散到 FS 之外的无关层。
4. openat 用同一 metadata 完成权限、目录判断和 inotify 类型；create mode/owner 尽量在创建
   原子操作中传入，不新增 TOCTOU。
5. 覆盖 O_CREAT/O_EXCL/O_TRUNC/O_DIRECTORY/O_NOFOLLOW/O_PATH、rename/unlink 并发测试。

## 涉及文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/` 的 resolved/open 契约
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{path_ops,stable_node,paged_handle}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/openat.rs`
- inotify 与权限适配测试

## CodeGraph 查询

```bash
codegraph explore "open_resolved_path_unchecked open_file open_stable_node check_existing_open_permission"
codegraph impact "VfsOpenOps"
codegraph callers "active_impl::backend().open"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# openat/openat2/create/excl/trunc/nofollow/inotify/rename-unlink race 回归
cd .. && git diff --check
```

普通 existing-file open 最终节点只进行一次 lookup/getattr 和一次 stable open，不再调用 exists；
功能回归通过后用任务 00 runner A/B。

## Commit 与简报

提交建议：`[perf] openat 复用稳定路径解析结果`。新增 `history/10-brief.md`。
