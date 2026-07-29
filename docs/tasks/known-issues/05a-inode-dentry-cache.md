# K-05A：稳定 inode 句柄与 dentry cache

## 任务目标

在 K-04 证明 path lookup 占比高后，让已打开文件的 range I/O 使用稳定对象身份，并
建立具备完整失效规则的 dentry/inode cache。该任务与 K-05B 可并行，但须先冻结
cache key 和 mount generation 契约。

## 执行前必读

- `docs/tasks/known-issues/05-fs-vfs-performance.md`
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

交付 `docs/tasks/known-issues/results/k05a-YYYYMMDD.md`。
