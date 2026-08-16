# Task 01：实现 VFS 目录项 / 负目录项缓存核心

## 任务目标

在 VFS 层增加类似 Linux dcache 的目录项缓存，使重复路径查找不再每次都进入
ext4 后端。

## 实施方案

1. 设计 dentry key：

   ```text
   (parent inode, name, mount)
   ```

2. 缓存 positive/negative entry：

   - positive：目标 inode/metadata 快照
   - negative：确认不存在的 name

3. 失效接口：

   ```text
   invalidate_dentry(parent, name)
   invalidate_mount(mount)
   ```

4. `rename/unlink/mkdir/rmdir` 调用精确失效，不清理全表。
5. 容量上限、哈希桶和简单 LRU 按当前 VFS 风格实现，避免无界增长。

## 涉及文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/**`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/**`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/**`

## CodeGraph 查询

```bash
cd /tmp/wateros-singlecore-compile-vfs
codegraph explore "active_impl backend metadata"
codegraph impact "VfsBackend"
codegraph callers "renameat2"
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

运行时至少：

- RV/LA 各 1 轮 BuildStorm；
- rename/unlink 后旧路径不可见，新路径立即可见。

## 完成后

新增 `history/01-brief.md`。

