# Task 02：statx/openat/readlinkat 共享 dcache 命中路径

## 任务目标

让 `statx`、`openat`、`readlinkat` 复用 Task 01 的目录项缓存，减少单核编译中的
重复路径解析和 ext4 查询。

## 实施方案

1. 在 `sys_statx` 中增加 cache 查找：

   - 命中 positive 时直接填充 `statx`
   - 命中 negative 时返回 `ENOENT`

2. `openat` 查找目标 inode 时优先命中 dcache。
3. `readlinkat` 缓存短 symlink 目标，失效点与 dentry 相同。
4. 保留 `AT_SYMLINK_NOFOLLOW`、`AT_EMPTY_PATH` 语义。

## 涉及文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fstat.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/openat.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/**`

## CodeGraph 查询

```bash
cd /tmp/wateros-singlecore-compile-vfs
codegraph explore "sys_statx sys_openat sys_readlinkat"
codegraph impact "resolve_path_at"
codegraph callers "backend.metadata"
```

## 验收方式

```bash
cd /tmp/wateros-singlecore-compile-vfs/os
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

运行时至少 RV/LA 各 1 轮 BuildStorm，确认无 `ENOENT` 误报、无旧路径可见。

## 完成后

新增 `history/02-brief.md`。

