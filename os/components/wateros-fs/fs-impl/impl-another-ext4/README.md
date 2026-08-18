# impl-another-ext4

[返回 wateros-fs](../../README.md) · [rootfs](../../fs-rootfs/README.md) · [VFS](../../../wateros-vfs/README.md)

这是当前默认 ext4 RW backend。它把 `another_ext4` 库接到 WaterOS `FsImpl/ReadOnlyFs/ReadWriteFs` 契约，并补充块设备适配、路径缓存、open-unlink 生命周期和错误持久化。

## 代码地图

| 文件 | 职责 |
| --- | --- |
| `backend.rs` | `AnotherExt4Impl` supported/probe/mount_ro/mount_rw |
| `block_io.rs` | WaterOS block device 到库 I/O trait，superblock magic probe |
| `filesystem.rs` | `AnotherExt4Fs` 全部长期状态与 orphan 管理 |
| `operations.rs` | RO/RW trait 方法、mount/sync/open/read/write/metadata 组合 |
| `path_lookup.rs` | ext4 inode lookup 与 metadata/error 转换 |
| `positive_dentry_cache.rs` | 固定容量 second-chance 正缓存 |
| `dentry_cache.rs` | 4096 项、4-way 负缓存与子树失效 |

## 核心状态

`AnotherExt4Fs` 保存：底层 `Ext4`、共享 block device、sticky I/O error flag、正/负 dentry cache、open inode refcount、orphan inode/隐藏链接、延迟 reclaim 和 orphan dir inode。

`check_backend()` 在每个公开操作前读取 sticky I/O error。块适配器一旦观察到 I/O 失败，应让后续操作继续报告错误，不能在上层把损坏状态当偶发成功。

## probe 与 mount

`probe` 只读 superblock magic，识别后返回 `FsKind::Ext4`；它不等同于完整 mount 校验。RO/RW mount 分别创建独立 `AnotherExt4Fs` 并封装为共享 LocalFs/LocalRwFs。

若同一块设备同时建立 RO 与 RW instance，缓存一致性依赖 backend/调用约束。rootfs 当前需要两种 handle，修改底层时必须验证 RW 更新后 RO 读取可见，且不会有两份相互覆盖的 metadata cache。

## dentry cache 不变量

正缓存以 path→inode 建索引，固定容量使用 clock/second-chance；entry 与 slot 必须双向一致。负缓存按 FNV-1a hash 分 1024 bucket、每 bucket 4 way。

所有 namespace 修改必须失效正确范围：

| 操作 | 正缓存 | 负缓存 |
| --- | --- | --- |
| create/link | 插入新 path | 删除 exact negative |
| unlink | 删除 path/subtree | 使后续 NotFound 可缓存 |
| mkdir | 插入目录 | 删除目标 negative |
| rename | 移动 old subtree 到 new；清 stale new | 删除 new subtree negatives |
| truncate/write | path→inode 不变 | 通常不变 |

若磁盘操作成功但缓存更新遗漏，会出现“第一次 ENOENT 后永远不存在”或 rename 后旧路径仍命中。缓存只能优化 lookup，不能成为 inode 生命周期真相源。

## open + unlink

POSIX 允许打开文件被 unlink 后由现有 fd 继续访问。本实现使用隐藏 orphan dir/link 保住 inode：

```text
open inode refcount > 0
  -> user unlink 前建立隐藏硬链接
  -> 提交用户可见 unlink
  -> fd 仍通过 inode 操作
  -> 最后 close 删除隐藏链接并 flush
  -> 删除失败加入 pending_reclaims
  -> 后续 sync/final close 重试
```

不能在用户 unlink 已成功后因为隐藏链接 reclaim 失败向用户回滚 namespace；此时只能记录并延迟回收。mount 时清理遗留 orphan 目录项，避免崩溃留下永久 inode。

## 写回与错误定位

```text
VFS paged handle writeback(offset, bytes)
  -> ReadWriteFs::write_at
  -> another_ext4 inode/extent update
  -> BlockAdapter read/write
  -> 可选 block cache
  -> VirtIO block
  -> sync/flush_all
```

日志至少保留 path/inode、offset/len、operation 和原始 block/backend error。`AccessViolation` 更可能来自 VFS/MM handle 权限/生命周期，`Io` 才继续沿 ext4/block 层定位；不要无条件互换。

## 回归

覆盖 superblock probe、mount RO/RW、跨块非对齐读写、sparse/truncate、mkdir/readdir、link/symlink/readlink、rename 覆盖、open-unlink-close、cache negative→create、子树 rename、fsync 与重启读回。错误注入要验证 sticky I/O error 和 pending orphan reclaim 不造成死锁或无限增长。
