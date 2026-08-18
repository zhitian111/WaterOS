# 全局文件页缓存实现手册

[VFS 总览](../../README.md) · [FS Bridge](../impl-fs-bridge/README.md)

该 crate 提供固定容量、全局共享的普通文件页缓存。key 为 `FileCacheKey { mount_gen, stable, path }`：有稳定节点时用 `(mount_id,node_id)` 判等，无稳定身份时才用路径。它是 VFS 文件内容缓存，不是 MM 的只读 ELF/mmap 物理页缓存。

## 数据结构

`GlobalFilePageCache` 持有文件条目表、固定页帧状态、mount generation 与 open-ref 计数。每个文件条目保存 logical size 和 `dirty_pages: page_index -> version`；`GlobalCacheState` 的每个 slot 保存 key、页号、数据、dirty/version 及 intrusive LRU 链。clean 与 dirty 分开维护 LRU，优先选择可安全复用的槽。

容量、页大小和预取 stride 来自 `wateros-base-config`；写回将至多 64 个连续页合并成一次 run。

## miss、写入与版本化写回

```text
read miss
  -> state 锁内选 slot/记录候选
  -> 释放 state 锁
  -> PageCacheIo::read_range
  -> 再加锁；若 peer 已装页则丢弃重复结果，否则安装

write
  -> 确保页已安装（部分页写需要旧内容）
  -> 修改缓存数据
  -> mark_dirty 生成非零 version
  -> dirty_pages[page] = version，更新 logical size

flush/dirty eviction
  -> 锁内复制待写数据和 expected version
  -> 锁外 write_range
  -> 再加锁，仅当 key/page/version 仍相同才清 dirty
```

最后一步的版本比较阻止旧写回覆盖/清除并发新写。写回失败必须保留 dirty 状态以便重试；不能为了腾 slot 直接丢弃失败的 dirty victim。

## 锁序与禁止事项

固定锁序为：`files` 短锁 → per-file `RwLock` → `state` 短锁 → 锁外下层 FS I/O。禁止持 `state` 调块设备；禁止持根卷 FS 锁反向获取 entry；禁止在等待 cache slot 时持调用方 inode/目录锁。违反会在 forkheavy/并发写回下形成长时间地址空间销毁或系统死锁。

`reset_global_cache` 在 mount generation 变化时原地清空索引并复用已分配帧池，避免反复分配大块内核 heap。切换前调用方必须先完成需要的 `flush_all`；reset 本身不应被当作“自动可靠持久化”。旧代次请求会被忽略，不能把全局 cache 倒退。

## 生命周期操作

- `acquire_open_ref_key`/`release_open_ref_key` 保护打开文件的条目与路径元数据；prepared read 也要持临时引用并在 finish/cancel/Drop 释放。
- `purge_closed_file` 仅在没有 open ref 时删除条目和槽。
- `truncate` 更新 logical size、删除尾部 dirty/index 页，并正确处理最后一个部分页。
- `finish_rename` 迁移无稳定身份的路径 key/open refs；稳定 node key 不应因路径变化失去身份。

## 回归矩阵

- hit/miss、跨页读写、短底层读和文件尾零填充。
- 同一页并发 miss，只安装一份且 duplicate load 可回收。
- flush 同时发生新写：旧版本写回不能清除新 dirty。
- dirty eviction 写失败后数据仍可重试；clean eviction 可复用。
- truncate 到页中/页界、rename、unlink-open、最后 close purge。
- mount generation 前进与 stale 请求，重复 reset 不增长大块 heap。
- 压力运行时同时观察 dirty 数、cache 命中/淘汰、FS writeback 错误和内核 heap；大量写回 warning 不能仅通过增大 RAM 掩盖。

