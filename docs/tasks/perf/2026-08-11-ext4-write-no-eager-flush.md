# ext4 普通写路径延迟 flush_all（2026-08-11）

## 为什么选择这里

当前 main 完整 BuildStorm 约 `817.27s`，Linux baseline 约 `395.90s`。最近一次完整
采样和 roadmap 都把普通文件写路径列为高优先候选：

```text
page-cache writeback
  -> FsPageIo::write_range
  -> StableNodeLease::write_range
  -> AnotherExt4Fs::write_range_node
  -> write_with_ordered_size
     -> fs.write(...)
     -> fs.flush_all()              # 每次 range write 都刷整个 another_ext4 cache
```

`another_ext4` 的 `BlockCache` 本身是 write-back + LRU eviction 写回实现：

- `write_block` 只把脏块放进内存 cache；
- 发生 cache set 替换时会写回被替换的脏块；
- `Ext4::sync/flush_all` 才主动刷全部脏块。

因此每次普通写后调用 `flush_all` 并不是“落盘一次”，而是把当前全部脏块重复写回，既增加
VirtIO 写放大，也破坏 page-cache writeback 本可以积累的批量写机会。Linux 的 buffered
write 同样不会在每次 `write(2)` 后立即刷盘，而是依赖 dirty page/writeback 与 `fsync`。

## 优化方案

1. 修改 `write_with_ordered_size`：只负责“更新 i_size + 写入 another_ext4 write-back
   cache”，不再调用 `fs.flush_all()`。
2. 保留显式持久化边界：
   - `ReadWriteFs::sync()` 仍调用 `flush_all()`；
   - `truncate/mkdir/unlink/rename/hardlink/chmod/chown/mknod` 等元数据操作仍按现状
     `flush_all()`；
   - `write_regular_file_at_root` 这类启动期显式创建文件仍按现状在写入后 `flush_all()`；
   - 页缓存 writeback 后由 cache eviction 自然写回，close/fsync 由
     `PagedFileHandle::flush/sync_dirty` 强制落盘。
3. 不改变 `api-v0`、文件系统 trait 或块设备 trait。
4. 不实现后台 writeback 线程：当前 LRU eviction 已经提供有界写回，先验证去掉 eager
   flush 本身是否有净收益；若脏块上限或可见性有问题，再补显式 dirty 水位。

## 为什么这么做

这是从“每次写都全盘持久化”过渡到 Linux 风格“buffered write + 有界 writeback + 显式
fsync”的最小改动。它不需要异步块 I/O，也不需要 IRQ，可以在当前同步
`SharedBlockDevice` 模型下直接验证。风险点主要是“不 fsync 时数据不保证立即落盘”，但
这正是 Linux 普通 buffered write 的语义，BuildStorm 测试以运行期构建结果为主，不依赖
每次写后掉电一致性。

## 下一步

1. 实现并运行双架构 `make check`。
2. 运行文件系统定向 smoke：根卷挂载、cagent 写读、BuildStorm 进入编译。
3. 跑完整 BuildStorm A/B；若相对 main 至少 1.5% 净改善则合并，否则回退并保留记录。
4. 用 pc-hot/wait-hot 对比 VirtIO/block cache 与脏页写回路径的指令变化。

## 验证结果

- 双架构 Final `make check` 通过。
- 180 秒 Final smoke 通过：根卷挂载、VFS 自检、cagent 全部通过，并进入 BuildStorm。
- 完整 BuildStorm（只去掉 `write_with_ordered_size` 的 eager flush）：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=807.79`，相对当前 main 基线
  `817.27s` 快约 1.16%。
- 追加普通 `truncate` 延迟 flush 后复测：`elapsed_s=817.18`，没有继续收益，已回退
  truncate 改动。
- 300 秒 pc-hot A/B 未显示早期 VirtIO/block-cache/TLSF 明显下降；`VirtQueue
  add_notify_wait_pop`、TLSF allocate/deallocate 与 block cache read 反而略高于同窗口
  main 基线，说明 807.79s 的改善在运行噪声范围内。

## 结论

当前改动不能稳定达到 1.5% 合并门槛，代码已全部回退，只保留本记录。后续若继续减少
ext4 写放大，应先统计 dirty block 数量、flush 次数与每次 flush 实际写回块数，再决定
是否引入按 inode/按 dirty-range 的定向 flush，而不是简单地删掉整文件系统 `flush_all`。
