# Task 00：固化 frame 分支基线并梳理缓存失效不变量

## 任务目标

确认本分支起点为 `perf/frame-allocator-pcpu`，并写清后续 VFS 缓存必须满足的
Linux 可见性/失效不变量。

## 实施方案

1. 记录 frame 分支 RV/LA 16G 中位数作为基线。
2. 保留基线内核：

   ```bash
   mkdir -p /home/zhitian/project/WaterOS_refactor/os/.perf-baseline-pre-singlecore
   cp /tmp/wateros-frame-allocator-pcpu/os/kernel-rv-final \
      /home/zhitian/project/WaterOS_refactor/os/.perf-baseline-pre-singlecore/kernel-rv-final
   cp /tmp/wateros-frame-allocator-pcpu/os/kernel-la-final \
      /home/zhitian/project/WaterOS_refactor/os/.perf-baseline-pre-singlecore/kernel-la-final
   ```

3. 列出缓存失效事件：

   - `rename/renameat2`
   - `unlink/unlinkat`
   - `mkdir/rmdir`
   - `mount/umount`
   - `chdir/chroot`（影响相对路径解析）
   - 文件内容/元数据发生写回

4. 明确缓存不得使用纯 TTL 代替精确失效。

## 涉及文件

- `docs/agents/tasks/singlecore-compile-vfs/README.md`
- `os/components/wateros-vfs/**`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/**`

## CodeGraph 查询

```bash
cd /tmp/wateros-singlecore-compile-vfs
codegraph explore "renameat2 unlinkat mkdir rmdir mount"
codegraph impact "VfsBackend"
codegraph callers "metadata"
```

## 验收方式

```bash
cd /tmp/wateros-singlecore-compile-vfs/os
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
git diff --check
```

## 完成后

新增 `history/00-brief.md`。

