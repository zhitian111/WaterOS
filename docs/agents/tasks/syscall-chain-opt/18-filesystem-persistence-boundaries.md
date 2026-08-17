# 任务 18：把持久化边界收口到 fsync/sync/O_SYNC

## 任务内容与目标

在任务 17 已证明 batch 正确后，移除普通异步 writeback 的全 FS 持久化，把设备 flush 与
journal/metadata commit 收口到 fsync、fdatasync、sync、O_SYNC/O_DSYNC 和 unmount。此任务
直接影响崩溃一致性，任何不确定错误语义都应回退。

## 实施方案

1. 明确三层状态：VFS dirty page、FS/backend dirty metadata/data、block-device durable。
2. 普通 writeback 只把页面提交给 FS backend；fsync 等边界按顺序完成 page cache flush、
   FS commit、device flush。
3. fdatasync 可只省略与数据无关 metadata，若后端无法区分则安全退化为 fsync。
4. O_SYNC/O_DSYNC 写成功前完成对应持久化；close 不能被无意扩大成 fsync。
5. delayed backend error 必须在后续 fsync/sync 返回，不能静默丢失。

## 涉及文件

- `os/components/wateros-fs/fs-api/api-v0/src/traits.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/{operations,path_lookup}.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs` 的 sync/fsync 入口

## CodeGraph 查询

```bash
codegraph explore "fsync fdatasync sync_dirty flush_all device.flush O_SYNC"
codegraph impact "ReadWriteFs::sync"
codegraph callers "sync_dirty"
```

## 验收方式

```bash
cd os
make rv_check && make la_check && make kernel-rv-final
# fsync/fdatasync/sync/O_SYNC/close/ENOSPC 定向回归
cd .. && git diff --check
```

QEMU 性能运行必须保留 `-snapshot`。崩溃/持久化验收另用可丢弃镜像副本，覆盖写后断电点、
重挂载内容与 `e2fsck -fn`；至少重复三轮。任一 silent data loss、目录损坏或延迟错误丢失即回退。

## Commit 与简报

提交建议：`[perf] 收口 another-ext4 持久化边界`。新增 `history/18-brief.md`，完整记录 crash
matrix、e2fsck 和性能 A/B；没有这些证据不得标记完成。
