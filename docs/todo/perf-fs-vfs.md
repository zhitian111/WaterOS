# 性能优化：文件系统与 VFS（页缓存 / 块缓存 / ext4 / flush / 回收）

## 用途

汇总 `wateros-fs` + `wateros-vfs` + `driver-block` 缓存层的性能瓶颈与资源回收/flush 隐患，重点是：页缓存 LRU 与脏页写回、块设备缓存、ext4 整文件读、VFS 路径解析与 dcache 缺失、sync/fsync 链路、文件 close/unlink 回收。这是「资源回收和 flush」需求的核心区域。

## 事实来源

- 代码静态链路分析；日志佐证 `[paged_handle] detached buffer cap exceeded` 等。
- 关联子链路分析见 [fs-vfs-subagent](fcb92735-08b2-4ca4-9db7-9f165361f9f5)。
- 交叉参考：`docs/audits/resources/page-cache.md`、`block-cache.md`、`docs/audits/locks/page-cache.md`、`docs/audits/locks/shared-fs-handles.md`、`docs/audits/resource-inventory.md`；测例缺口 `os/ltp_log/todo/vfs_io.md`。
- 容量基线：页缓存 4096×4KiB≈16MiB；块缓存 64 槽（RV）。

## 覆盖范围

`os/components/wateros-vfs/vfs-impl/impl-page-cache`、`impl-fs-bridge`、`os/components/wateros-fs/fs-impl/impl-ext4`、`fs-rootfs/rootfs-impl/impl-kernel`、`fs-procfs`、`os/components/wateros-driver/driver-block/block-impl/impl-block-cache`、`wateros-vfs/src/{fd.rs,cwd.rs}`。

---

## 优化点清单（按预期收益从高到低）

### F-1. AuxRo 卷 `BufferedFileHandle` + ext4 `read()` 整文件读入堆 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/file_handle.rs:56-60,293-294`、`os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs:553-581`
- **当前实现/复杂度**：AuxRo 在 `open_path` 走 `BufferedFileHandle::open`，open 内调 `bridge.read()` → ext4 `read()` 按 `file_size` 分配 Vec 并循环 `read_bytes`（chunk≤512B），O(file_size) 内存 + O(file_size/512) 次 ext4 读。根卷已统一 `PagedFileHandle`，但 AuxRo 仍整文件读入堆；`FILE_LARGE_THRESHOLD`（`base-config/fs.rs:9`）已定义却未被引用。
- **问题**：大文件或频繁 open 触发 `detached buffer cap exceeded` 同类问题（Buffered 无硬 cap）。
- **改进方案**：AuxRo 与根卷对齐统一 `PagedFileHandle` + 页缓存（RO 只读 install）；或至少 `read_range` 流式打开，open 不读内容。
- **预期收益**：高，open 从 O(size) 降为 O(1)，消除 aux 卷大文件 OOM/堆压力。
- **架构差异**：两架构相同。
- **风险/依赖**：独立 RO 卷需确认只读页缓存不写回语义。

### F-2. `unlink_path` → `purge_closed_file` 无 flush，脏页静默丢弃 【高 / 正确性 P0】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs:432-445`、`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:751-787`
- **当前实现/复杂度**：unlink 成功后立即 `purge_closed_file`：移除 `open_refs`/`files`，扫描整个 `state.index` 过滤 `(key==path)` 回收帧（O(全局 resident 页数)），不调用 writeback/flush。
- **问题**：打开中 fd 或未 fsync 的脏页被直接丢弃 → 数据丢失 + 无意义后续 ext4 I/O。
- **改进方案**：purge 前 `flush(path)`；或 `open_refs>0` 时 defer purge（Linux unlink 语义）；purge 时对 dirty 页强制写回或拒绝。
- **预期收益**：高（正确性 P0），避免丢刷后重复写/读不一致。
- **架构差异**：无。
- **风险/依赖**：与 unlink 错误码（EBUSY）语义对齐。

### F-3. ext4 每次 I/O 全路径 `path_to_inode`，无 dcache / inode 缓存 【高】

- **位置**：`os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs:329-332,537-540,584-601`
- **当前实现/复杂度**：每次操作 `Path::try_from` → `fs.path_to_inode(pathv, FollowSymlinks::All)` → 再 metadata/open_inode；路径深 D、每级目录平均 E 项 → O(D×E) 目录查找，无 VFS dcache/inode 缓存。
- **问题**：页缓存 miss 时 `install_page` → `FsPageIo::read_range` → 又一次全路径 walk + FS 全局 Mutex；stat/open/read 小文件重复解析。
- **改进方案**：VFS 或 ext4 层加 `(mount_id, path) → inode` 的 LRU dcache；open 时缓存 inode 号到 `PagedFileHandle`；symlink 解析结果缓存。
- **预期收益**：高，减少 syscall 密集场景的 FS 锁持有与目录扫描。
- **架构差异**：无。
- **风险/依赖**：rename/unlink/mount 须失效 dcache；与页缓存 `(mount_gen, path)` 键协调。

### F-4. 页缓存 LRU `touch_lru` / `detach` 线性扫描 VecDeque 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:84-93,123-128,772-777`
- **当前实现/复杂度**：每次命中/驱逐 `lru.iter().position(|x| idx)` → `remove(p)` → `push_back`，O(capacity)，capacity=4096。**已抽查确认** `touch_lru` 为 `iter().position` 线性扫描。读/写/flush 每页至少 1 次 touch。
- **问题**：4096 帧下最坏约 4096 次比较/touch，与 BTreeMap O(log n) 索引不对称，热路径 CPU 与 `state` Mutex 临界区偏长。
- **改进方案**：经典 O(1) LRU（intrusive 双向链表 + 槽位数组存 prev/next）；或 Linux 式 active/inactive 双链表 + 批量老化。
- **预期收益**：高，页缓存命中/写路径 CPU 与锁竞争显著下降。
- **架构差异**：无。
- **风险/依赖**：保持 index/free/lru 不变量；与 `purge_closed_file` 批量移除兼容。

### F-5. 页缓存 flush / 驱逐写回长时间占用 `SharedRwFs`，整卷 FS 自旋阻塞 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:314-394,668-729`、`impl-fs-bridge/src/paged_handle.rs:60-84`
- **当前实现/复杂度**：`flush` 按脏页 BTreeMap 分批（`FLUSH_RUN_MAX_PAGES=64`）`flush_dirty_run` → `io.write_range`；Root 路径每 chunk 释锁，但 AuxRw 单次 `fs.lock().write_range` 写整 batch。O(脏页数) 次 ext4 写 + 锁竞争。
- **问题**：大文件 fsync/close 期间其他任务 `exists/mkdir/read` 需 spin 等 `SharedRwFs`（SFH-01），LTP 多线程表现为整卷卡死。
- **改进方案**：Aux 与 Root 对齐分段释锁；batch 间 yield；合并相邻脏页后单次 ext4 写；长期 inode 级写回绕过路径解析。
- **预期收益**：高，降低 fsync/close 对系统吞吐的全局停顿。
- **架构差异**：无。
- **风险/依赖**：缩短 batch 会增 ext4 调用次数，需实测平衡点。

### F-6. `global_cache(mount_gen)` 每次 I/O 静态 Mutex + alias bump 不 flush 致丢脏 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:855-887,196-211`、`os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs:135,168`
- **当前实现/复杂度**：每次 read/write/open/flush `GLOBAL_CACHE.lock()`；`mount_gen > current` 时 `reset_to_gen` 原地清表（注释要求先 flush，但 alias bump 路径未保证）；`PagedFileHandle` 每次调 `global_cache(self.mount_gen)`。
- **问题**：热路径额外全局锁；mount 代次漂移时静默丢弃脏页；stale gen 返回新缓存致 `open_refs` 键错位。
- **改进方案**：open 时缓存 `Arc<GlobalFilePageCache>` 到句柄；所有 `bump_mount_generation` 统一走 `mount_table.rs:132-137` 的 `reset_file_page_cache`；stale 句柄返回 EBADF。
- **预期收益**：高，热路径去锁 + 消除 mount 丢脏。
- **架构差异**：rootfs alias bump 两架构相同。
- **风险/依赖**：审计所有 bump 调用点。

### F-7. `sync(2)` 仅刷已打开 fd，不覆盖「无 fd 脏页」与块层 【高】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/sync.rs:26-29`、`impl-fd-session/src/registry.rs:342-353`、`impl-block-cache/src/lib.rs:81-84`
- **当前实现/复杂度**：`sys_sync` → `flush_all_open_files` 遍历 fd 表 `handle.flush()`，不调 `GlobalFilePageCache::flush_all`；块缓存 `flush()` no-op（写穿）；fsync/fdatasync 同路径未区分 metadata。
- **问题**：仅 metadata 触发的脏页、crash 前未 close 的缓存 sync 不可见；fdatasync 未跳过 metadata；`/dev/null` 等 fsync 行为见 `vfs_io.md`。
- **改进方案**：`sys_sync` 增 `page_cache.flush_all` + 可选块设备 barrier；fdatasync 仅刷数据页；特殊 fd 返回 EINVAL。
- **预期收益**：高，正确性 + 测试通过率。
- **架构差异**：无。
- **风险/依赖**：全缓存 flush 耗时长，需与 F-5 分段释锁一并做。

### F-8. `install_page` 每次 miss 堆分配 4KiB + 驱逐脏页 `data.clone()` 【高】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:419-461,129-131`
- **当前实现/复杂度**：miss 时 `vec![0u8; FILE_PAGE_SIZE]` 临时缓冲 → 锁外 `read_range` → 抢槽；驱逐脏页 `frames[idx].data.clone()` 用于锁外写回。高 churn 每 miss 一次 alloc，每驱逐脏页 +4KiB clone。
- **问题**：128MiB 堆下大量随机读/多文件交替致分配器压力与碎片；clone 倍增内存带宽。
- **改进方案**：复用帧内 `PageFrame.data` 作读缓冲（双缓冲或 in-place read）；驱逐写回直接用 `frames[idx].data` 切片，成功后清 dirty 复用槽。
- **预期收益**：高，降低 miss/驱逐路径堆 churn 与拷贝。
- **架构差异**：无。
- **风险/依赖**：与「锁外 I/O 不持 state」约定一致。

### F-9. LoongArch64 未启用 `BlockCacheManager::wrap`，块缓存热路径缺失 【高（LA）】

- **位置**：`os/components/wateros-driver/driver-impl/impl-qemu-loongarch64-virt/src/lib.rs:131-134`（直接 `Arc<Mutex<Box<dyn BlockDevice>>>`）；对比 RV `impl-qemu-riscv64-opensbi` 使用 `BlockCacheManager::wrap`。
- **当前实现/复杂度**：LA 每次 ext4 块读直达 VirtIO；RV 有 64 槽 LRU + 连续 miss 合并读。
- **问题**：同代码库双架构 I/O 性能不一致；LA ext4 热路径多一次 VirtIO 往返/块。
- **改进方案**：LA probe 与 RV 对齐，`cfg(feature="block-cache")` 下 wrap。
- **预期收益**：高（LA 平台），RV 无变化。
- **架构差异**：RV 有 64 槽块缓存，LA 当前无。
- **风险/依赖**：PCI virtio-blk 路径已存在；32KiB 固定开销可忽略。

### F-10. `purge_closed_file` 全表 index 扫描 + 每槽 LRU 线性移除 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:761-786`
- **当前实现/复杂度**：`index.keys().filter(|(k,_)| *k==key).collect()` O(N)（N=全局 resident 页≤4096）；每移除槽 `lru.iter().position` O(capacity)；批量 purge 最坏 O(N×capacity)。
- **问题**：close/unlink 回收在饱和时延迟明显；应为 O(该文件页数)。
- **改进方案**：`FileEntryInner` 维护 `resident_pages: BTreeSet<u64>` 或 reverse index `key → Vec<page_idx>`，purge 只遍历文件页列表。
- **预期收益**：中，回收延迟从 O(全局) 降到 O(文件页数)。
- **架构差异**：无。
- **风险/依赖**：install/evict 须同步维护 reverse index。

### F-11. `AUX_MOUNTS` 最长前缀挂载线性扫描 + open_path 双重 resolve 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs:99-119,171-189,224-247`、`file_handle.rs:265,289`
- **当前实现/复杂度**：`longest_aux_mount` 持锁遍历 `Vec<MountEntry>` 取最长前缀 O(M)；每次 resolve_route/open_path/exists 至少 1 次；`open_path` 两次 `resolve_route`。
- **问题**：挂载点增多（tmpfs/cgroup/proc/bind）时每次路径解析重复扫描。
- **改进方案**：挂载点按路径长度排序的 trie/前缀树；缓存上次 `(prefix → FsRoute)`；合并 open_path 双重 resolve。
- **预期收益**：中，多 aux 挂载 + 高频 open。
- **架构差异**：无。
- **风险/依赖**：mount/unmount 须更新索引。

### F-12. `EXT4_SMALL_READ_CACHE` 全局单块缓存 + 双锁交叉 + partial write 堆分配 【中】

- **位置**：`os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs:24-88,156-198,211-212`
- **当前实现/复杂度**：全局 `Mutex<SmallReadCache>` 缓存 1 个 `(dev_id, block)`，≤64B 单块内命中 O(1)；写路径持 `dev.lock()` 全程，头尾 RMW `write_partial_block` 再堆分配 Vec。
- **问题**：多 FS 实例下可能脏读（SFH-02）；与 64 槽块缓存、4096 页缓存三层叠加一致性复杂；partial write 每次 `vec.resize(bs)` alloc。
- **改进方案**：缓存移入 `Ext4FsRw` 实例或依赖块缓存去掉小读缓存；partial 用栈上 `[u8; BLOCK_SIZE]`。
- **预期收益**：中，减少全局锁与 alloc，简化缓存层次。
- **架构差异**：无。
- **风险/依赖**：单 RW 根主路径下 SFH-02 不可达，RO+aux RW 边缘路径仍存在。

### F-13. 块缓存 `touch_lru` O(n) 与写后不填充冷块 【中】

- **位置**：`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs:86-91,182-198`
- **当前实现/复杂度**：64 槽 `touch_lru` 线性扫描 O(64)；`write_blocks` 写穿 inner，仅更新已在 map 中的 LBA，未缓存块不 `cache_put`。
- **问题**：写后读冷块需再次 VirtIO 读；touch 在满槽时频繁触发。
- **改进方案**：O(1) LRU（同页缓存）；写穿后选择性 `cache_put`（write-allocate）提升读-after-write。
- **预期收益**：中，写密集后读、metadata 读路径减少底层 I/O。
- **架构差异**：RV 有块缓存；LA 见 F-9。
- **风险/依赖**：写分配略增内存占用（仍固定 64 槽）。

### F-14. 顺序读预取 `FILE_READ_AHEAD_STRIDE=8` 对随机读产生无效 I/O 【中】

- **位置**：`os/components/wateros-base/base-config/src/fs.rs:16-19`、`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:565-574`
- **当前实现/复杂度**：每次 read 完成后预取后续 8 页（32KiB），每页 `install_page`（可能驱逐+写回）；随机读 O(8×install 成本) 无效 I/O。
- **问题**：LTP/多文件交替读致 LRU 抖动、脏页驱逐写回放大。
- **改进方案**：顺序检测（连续 page_idx）再预取；per-fd 预取状态；随机 workload 降为 0。
- **预期收益**：中，随机读降低无效块 I/O 与 LRU 压力。
- **架构差异**：无。
- **风险/依赖**：顺序 benchmark（lmbench）需保留预取。

### F-15. ext4 整文件 `read()` / procfs 全文件读仍被多路径间接使用 【中】

- **位置**：`os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs:553-581`、`impl-fs-bridge/src/lib.rs:256-280`、procfs `lib.rs:335-336`
- **当前实现/复杂度**：`ReadWriteFs::read` 分配 `file_size` Vec；procfs 每次 range 读先读全文件再切片。
- **问题**：任何调用 `SingleRootReadView::read` 的路径（含 BufferedFileHandle、部分 procfs）O(file_size)。
- **改进方案**：废弃整文件 `read` 热路径，统一 `read_range`；procfs 按 offset range 生成。
- **预期收益**：中，减少堆峰值与重复读。
- **架构差异**：无。
- **风险/依赖**：API 调用面 grep 确认。

### F-16. `get_file_entry` 持 `files` Mutex 内嵌套 `entry.write` 升级 size 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:218-226`
- **当前实现/复杂度**：`files.lock()` 内若 `initial_size > logical_size` 则 `entry.write()` 升级 size，延长 files 临界区，违反模块头「files 极短持锁」约定。
- **问题**：与 flush_all/logical_size_for_key 竞争，放大自旋。
- **改进方案**：files 锁内仅 insert/clone Arc；size 升级移到释锁后 `entry.write`。
- **预期收益**：中，降低 metadata 读与 I/O 并发时 files 锁等待。
- **架构差异**：无。

### F-17. `rename_path` 不迁移/失效页缓存键 【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs:567-577`
- **当前实现/复杂度**：rename 仅改 ext4 目录项；页缓存仍挂 `(mount_gen, old_path)`；已打开 fd 持旧 path。
- **问题**：新路径冷启动重复读盘；旧路径缓存 stale；长期 `files` 条目泄漏。
- **改进方案**：rename 时 migrate `FileCacheKey.path` 或 purge 旧键 + bump 代次；句柄 path 更新。
- **预期收益**：中，减少 rename 后双倍缓存与错误读。
- **架构差异**：无。
- **风险/依赖**：open fd 并发 rename 语义需定义。

### F-18. `PagedFileHandle` detached 模式堆缓冲（unlink 后写）【中】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs:27-37,113,212-236,286-304`
- **当前实现/复杂度**：下层 `NotFound` 转 detached，`detached_data` Vec 增长至 16MiB 上限；clone dup 时整 buf clone。
- **问题**：日志 `detached buffer cap exceeded`；dup 大 detached 文件 O(size) 复制。
- **改进方案**：unlink 后拒绝写或返回 EBADF；detached 用页缓存 detached 键而非堆；dup 共享 `Arc<Vec>`。
- **预期收益**：中，边缘路径堆安全；dup 降拷贝。
- **架构差异**：无。

### F-19. tmpfs `unlink` 不回收 inode 号 【低】

- **位置**：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/tmpfs.rs:81-84,305-313`
- **当前实现/复杂度**：`alloc_inode` 单调递增；unlink 只 `children.remove` 不复用 inode。
- **问题**：长跑大量 create/unlink（cgroup/tmpfs 测试）`next_inode` 无界增长。
- **改进方案**：freelist 回收 inode；或测试场景 periodic remount。
- **预期收益**：低，极端长跑才显现。
- **架构差异**：无。

### F-20. `drop_task_fd_table` / close flush 错误静默 【低（可观测性）】

- **位置**：`os/components/wateros-vfs/src/fd.rs:203-210`、`paged_handle.rs:366-369`
- **当前实现/复杂度**：任务退出 `drain_task_fd_table` → `let _ = handle.close()` 忽略 flush 错误；close 返回 sync_err 但 open_ref 已释放。
- **问题**：任务退出丢刷无日志。
- **改进方案**：close 失败 `log::warn!` 含 path/dirty_count；任务退出汇总 warn；可选强制 flush_all。
- **预期收益**：低，可观测性 + 边缘丢刷诊断。
- **架构差异**：无。

### F-21. 块缓存 `evict_lru_slot` invariant 失败 panic 【低（可靠性）】

- **位置**：`os/components/wateros-driver/driver-block/block-impl/impl-block-cache/src/lib.rs:97-101`
- **当前实现/复杂度**：`lru` 空或槽未占用时 `expect` panic。
- **问题**：不变量破坏致整机崩溃；写穿下本应可恢复。
- **改进方案**：返回 `DriverError::Internal` + warn + 安全清空 cache 重试。
- **预期收益**：低（可靠性 > 性能）。
- **架构差异**：无。

---

## 热路径链路摘要

读路径（根卷普通文件，Direct 模式）：

```
syscall read → fd.with_current_io → PagedFileHandle::read
  → global_cache() [GLOBAL_CACHE Mutex]
  → GlobalFilePageCache::read
      → get_file_entry [files Mutex]
      → install_page (每页) [state Mutex, touch_lru O(n), 可能驱逐+writeback]
          → FsPageIo::read_range → resolve_route O(M) → root_rw.lock
              → ext4 read_range → path_to_inode O(D×E) → read_with_small_cache / block dev
                  → CachingBlockDevice::read_blocks [dev Mutex, map O(log 64), touch_lru O(64)]
      → read_ahead 8 pages
```

flush / fsync 路径：

```
sys_fsync → with_current_io → PagedFileHandle::flush → sync_dirty
  → cache.flush → entry.write(收集 dirty) → flush_dirty_run(≤64页/batch)
      → FsPageIo::write_range → SharedRwFs.lock → ext4 write_range → block_write_bytes
sys_sync → 仅 fd 表 flush_all（不含未打开文件的页缓存）
```

## 架构差异速查

| 项 | riscv64 | loongarch64 |
|----|---------|-------------|
| 块设备 LRU 缓存 | `BlockCacheManager::wrap` 64 槽 | 未 wrap，裸 VirtIO |
| 页缓存 / VFS / 容量常量 | 相同 | 相同 |

## 落地优先级建议

1. F-2 / F-7 / F-6 flush 与丢脏正确性（unlink purge flush、sync 覆盖页缓存、mount alias bump flush）
2. F-4 / F-8 页缓存 O(1) LRU + 帧复用
3. F-1 / F-15 消除整文件读堆，统一 read_range
4. F-3 dcache / inode 缓存
5. F-5 flush 分段释锁；F-9 LA 块缓存接线
6. F-10~F-18 回收/锁/预取细化

## 后续维护入口

- 改页缓存：同步 `docs/audits/resources/page-cache.md`、`docs/audits/locks/page-cache.md`。
- 改块缓存/驱动接线：同步 `docs/audits/resources/block-cache.md` 与两架构 `driver-impl`。
- 改 ext4/VFS：同步 `docs/guides/filesystem-current.md`、`docs/exports/features/wateros-fs.md`/`wateros-vfs.md`。
