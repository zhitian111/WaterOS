# 任务 20：VFS metadata/read_range 使用稳定只读通道

## 任务内容与目标

把稳定普通文件的 metadata、cache miss read 和 lazy ELF fault 切换到任务 19 的并发 read
channel，减少多核 BuildStorm 在根 FS 全局 Mutex 上串行。所有路径 fallback、写回和目录变更
继续使用安全的独占路径。

## 实施方案

1. `StableNodeLease` 同时持有 node identity 与 read channel lease，不再每次 `fs.lock()`。
2. `metadata_node/read_range_node` 走只读通道；truncate/write/sync/close_node 保持独占接口。
3. unlink/rename 后 stable inode read 正确，最终 close 与 orphan reclaim 不和读路径死锁。
4. page cache miss 和 ExecFile fault 使用同一路径；命中页缓存时不进入 FS。
5. 加入并发 reader、writer、rename、unlink、exec fault 和 close 压力测试。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{stable_node,paged_handle}.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/`
- `os/components/wateros-mm/mm-impl/impl-{sv39,loongarch64}/src/kernel_elf.rs`
- FS read channel facade

## CodeGraph 查询

```bash
codegraph explore "StableNodeLease metadata read_range FsPageIo ElfPathSegmentLoader"
codegraph impact "StableNodeLease"
codegraph callers "read_range_node"
```

## 验收方式

```bash
cd os
make rv_check && make la_check
make kernel-rv-final && make kernel-la-final
# SMP read/write/rename/unlink/exec 压力与 BuildStorm
cd .. && git diff --check
```

任务 01/锁诊断证明多个稳定 read 可并发，root FS 独占锁等待下降；任务 00 runner 进行 8 核
交错 A/B。出现死锁、orphan 泄漏、陈旧 inode 或性能退化即回退。

## Commit 与简报

提交建议：`[perf] VFS 稳定读取使用根 FS 并发通道`。新增 `history/20-brief.md`。
