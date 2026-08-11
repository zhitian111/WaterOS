# K-05A：稳定 inode 句柄与 dentry cache

## 任务目标

在 K-04 证明 path lookup 占比高后，让已打开文件的 range I/O 使用稳定对象身份，并
建立具备完整失效规则的 dentry/inode cache。该任务与 K-05B 可并行，但须先冻结
cache key 和 mount generation 契约。

## 执行前必读

- `docs/tasks/known-issues/05-fs-vfs-performance/task.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-fs.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/todo/perf-fs-vfs.md`

## 已知信息与代码证据

another-ext4 当前每次 range read 都重新解析路径：

```rust
let inode = lookup(fs, path)?;
let attr = fs.getattr(inode)?;
fs.read(inode, offset as usize, buf)
```

这在 page miss、stat/open 和顺序读中可能重复，但收益必须由 lookup 计数确认。

## 当前进度（2026-08-03）

已完成第一阶段：another-ext4 适配层使用容量为 4096 的 path→inode cache，覆盖
mount、create、unlink/rmdir、rename、hardlink 和 mknod 的失效或迁移；range read
命中后直接使用 inode 读取。final 工具链微基准中 `rustc --version` 从约 11 秒降至
约 3.6 秒，详见
[`results/k05a-path-inode-cache-phase1-20260802.md`](./history/k05a-path-inode-cache-phase1-20260802.md)。

稳定节点实现已经分三次独立提交完成：FS `api-v0` 契约、another-ext4 inode/open
生命周期、VFS bridge identity page-cache key。打开句柄现在通过
`(mount generation, mount identity, FsNodeId)` 区分对象；hardlink 共享缓存，unlink 和
rename overwrite 后旧 fd 继续访问原 inode，稳定后端不再复制最大 16 MiB 的 detached
缓冲。初赛和新版决赛镜像的 8 核定向回归及写后 `e2fsck -fn` 已通过，详见
[`results/k05a-vfs-stable-node-20260803.md`](./history/k05a-vfs-stable-node-20260803.md)。

实现闭环已经完成；K-05A 总任务仍保持开放，等待 BuildStorm/LTP 全量回归和三轮性能
数据满足下方验收条件。

## 涉及文件

- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/{file_handle,paged_handle}.rs`

## 任务内容

1. 在 FS API 增加稳定 node/object handle 或 inode range I/O，不泄漏 another-ext4 类型。
2. 打开句柄保存 `(mount generation, object identity)`，read/write/metadata 优先走它。
3. 若增加 path cache，定义 rename、overwrite、unlink、hardlink、truncate、
   mount/unmount 和 inode reuse 的失效。
4. 保持 unlink 后已打开 fd 可访问原对象；path cache 不能替代 open-file lifetime。
5. 分开提交 API、another-ext4 impl、VFS bridge 和 cache policy。

## 如何验收

- [ ] path lookup 次数和 stat/open/read 延迟有稳定改善。
- [ ] rename overwrite、unlink-open-fd、hardlink、mount generation、inode reuse 测试通过。
- [ ] basic/BuildStorm/LTP FS 回归及 `e2fsck -fn` 通过。
- [ ] `make rv_check && make la_check` 通过。

交付 `docs/tasks/history/known-issues/k05a-YYYYMMDD.md`。
