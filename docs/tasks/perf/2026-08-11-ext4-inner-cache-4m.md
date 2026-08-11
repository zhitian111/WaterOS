# another_ext4 内层块缓存扩容（2026-08-11）

## 为什么选择这里

当前 pc-hot 中 `VirtQueue::add_notify_wait_pop` 约 `1.34B` 条指令，仍是内核最大可控
热点之一。之前已回退过外层 LBA block cache 16MiB 和页缓存 40MiB，因此不再继续堆外层
缓存。另一个没有测过的容量点是 `another_ext4` 自己的 write-back block cache：

```text
CACHE_SIZE = 64
CACHE_ASSOC = 4
总容量 = 64 * 4 * 4096 = 1MiB
```

BuildStorm 读取 ext4 元数据、inode、extent 与文件块；1MiB 内层缓存容量较小，可能有大量
重复块穿透到 WaterOS LBA block cache 和 VirtIO。

## 优化方案

把 `another_ext4` 的 `CACHE_SIZE` 从 `64` 调为 `256`，保留 `CACHE_ASSOC = 4`：

- 总容量约 `4MiB`；
- 不改变 LRU、write-back、COW snapshot 或 flush 语义；
- 不改变外层 LBA block cache 和 page cache 容量；
- 只在挂载时多分配约 4MiB 内核堆内存，BuildStorm 当前 128MiB 堆可以承受。

## 为什么这么做

这是对“三层缓存职责不清”的谨慎实验：先只验证内层 ext4 cache 容量是否仍是 VirtIO
重复读的瓶颈。若 VirtIO 明显下降且完整 BuildStorm 有净收益，再考虑按 metadata/data
分流；若没有收益，回退并停止继续扩大缓存容量。

## 下一步

1. 调整 `CACHE_SIZE` 并运行双架构 Final check/build。
2. Final smoke 覆盖根卷、cagent 读写和 BuildStorm 启动。
3. 完整 BuildStorm A/B 与 300 秒 pc-hot A/B。
4. 有效则合并 main，无效则回退并记录。

## 验证结果

- 双架构 Final `make check` 通过。
- Final smoke 通过：根卷、VFS 自检、cagent 全部通过，并进入 BuildStorm。
- 完整 BuildStorm：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=822.64`，相对当前 main
  `809.42s` 慢约 `1.63%`。
- 300 秒 pc-hot：
  - `VirtQueue add_notify_wait_pop` 约 `1.34B` -> `1.31B`（-2%）；
  - 总指令、memcpy、memset、TLSF allocate/deallocate 均高于当前 main。

## 结论

扩大 another_ext4 内层缓存只换来少量 VirtIO 下降，但缓存初始化和内存占用让整体指令与
墙钟变差。代码已回退，只保留本记录。VirtIO 热点不能通过继续扩大缓存容量解决，后续应
回到请求生命周期或批处理方向。
