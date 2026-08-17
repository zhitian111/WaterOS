# 任务 17：一个文件 writeback 周期只提交一次后端缓存

## 任务内容与目标

在不改变 fsync/close/O_SYNC 可见语义的前提下，把 another-ext4 每个最多 64 页 write batch
执行一次或两次 `flush_all()`，收敛为一个文件 `flush_key` 周期至多一次后端提交。本任务仍
保留 writeback 结束提交，不直接推迟到 fsync。

## 实施方案

1. FS API 增加明确的 writeback begin/write/commit 或 batch-write 窄契约；默认实现保持旧语义。
2. another-ext4 的 batch 内数据/size 更新不逐次 flush，最后一次 commit 执行 `flush_all()`。
3. 文件扩展保证 size、数据和元数据的有序性；错误时未写完页面不得在 VFS 中标 clean。
4. page cache `flush_key` 将多个 64 页 run 纳入同一文件周期，不能跨文件共享事务状态。
5. 增加短写、ENOSPC、扩展、覆盖写、commit 失败和重试测试。

## 涉及文件

- `os/components/wateros-fs/fs-api/api-v0/src/traits.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/{path_lookup,operations}.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/file_cache.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`

## CodeGraph 查询

```bash
codegraph explore "write_with_ordered_size flush_all flush_key writeback_dirty"
codegraph callers "write_with_ordered_size"
codegraph impact "PageIo::write_range"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-vfs/vfs-impl/impl-page-cache/Cargo.toml
cargo test --offline --manifest-path components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
cd .. && git diff --check
```

使用任务 00 的 `-snapshot` QEMU 做 iozone/BuildStorm；另用可丢弃 raw 镜像副本执行写入、
fsync、重挂载与宿主 `e2fsck -fn`。flush 计数应从每 batch 降为每 file cycle 一次。

## Commit 与简报

提交建议：`[perf] 合并单文件 writeback 后端提交`。新增 `history/17-brief.md`，必须附镜像
一致性结果和 flush 次数对比。
