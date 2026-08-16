# Task 09：双架构功能与资源安全终验

## 任务目标

在性能终验前证明最终候选没有 allocator、frame、远程释放、VFS、进程和镜像相关 bug。

## 验收方式

### 静态/构建

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
git diff --check
```

### 运行功能

使用任务 00 的干净脚本镜像，RV/LA 各至少三轮完整 BuildStorm；额外执行：

- slab alloc/free/realloc、跨 CPU free、远程队列超限、页回收压力；
- fork/exec/exit/wait、futex、mmap/mprotect、文件重开/fsync/卸载；
- 产物启动验证和 `e2fsck -fn` 只读检查。

每轮必须包含 `TOOLCHAIN_RESULT status=OK`、`MINIBUILD_RESULT status=OK`、
`BUILDSTORM_RESULT ... status=OK ... run=OK`、`all commands finished`，且无 panic、
OOM、ENOMEM、SIGSEGV、page fault、shootdown timeout、recursive heap allocation、
double free、UAF、丢失/重复 frame 或缓存失效异常。

## 涉及文件与 CodeGraph

本任务不新增生产代码，审计全部最终 diff：

```bash
codegraph impact "KernelAllocator SlabAllocator HeapFrameSource"
codegraph callers "dealloc_frame remote_push realloc"
```

## 完成后

新增 `history/09-brief.md`，记录每轮日志路径、功能检查、资源统计和所有未验证限制。

