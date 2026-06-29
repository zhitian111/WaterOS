# 性能任务：文件系统读路径优化（G2c~f）

## 任务目标

提升 **iozone 读** 与 **lmbench stat/open** 吞吐/延迟，使 score 有机会 **> 1.0**。在块缓存已启用前提下（见 `wave1-enable-block-cache.md`），实施 FS/VFS 侧改动。

**建议拆 PR**：dcache → ext4 读分片 → 页缓存 LRU → install_page/预取（可并行子 agent）。

## 背景（必读）

- `docs/todo/perf-baseline-gap-report.md` §G2.2~2.3
- `docs/todo/perf-fs-vfs.md`（F-3、F-4、F-8、F-16）

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的文档

- `docs/todo/perf-fs-vfs.md`
- `docs/todo/perf-risk-assessment.md`（F-3 中风险）
- `docs/exports/features/wateros-fs.md`、`wateros-vfs.md`（若存在）

## 需要优先查看的源文件

| 文件 | 改动点 |
|------|--------|
| `os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs:659-691` | 双 path walk、512B 分片 |
| `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs` | 句柄绑 inode、open 路径 |
| `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs:84-93,399-435,577-585` | O(n) LRU、install_page、预取 |
| `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs:291-294` | fd O(n)（open/close 顺带） |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/openat.rs`、`fstat.rs` | stat/open 路径 |

## 实施要点（可分步）

### A. dcache / inode 句柄（F-3，收益最大）

- VFS `(mount_gen, path) → inode` LRU；rename/unlink/mount 失效
- `PagedFileHandle` 存 `(inode_no, mount_gen)`，`read_range` 增 inode 级 API
- 删除 `read_range` 入口重复 `metadata` + 第二次 `path_to_inode`

### B. ext4 读放大（低风险）

- `read_range` 循环步长改为 `min(room, FILE_PAGE_SIZE)` 或 fs block_size，避免 512B×8

### C. 页缓存 O(1) LRU（F-4）

- 替换 `touch_lru` 的 `VecDeque` 线性扫描为槽位 + 侵入式链表

### D. install_page / 预取（F-4、F-14）

- miss 时直接用槽位缓冲，去掉 `vec![0;4096]` 与驱逐 `clone()`
- 顺序读：预取批量化或异步；随机读检测后 disable 预取

### E. （可选）fd 空闲位图（L-6）

- 改善 lmbench open/close，与 A 叠加

## 验收标准

- [ ] `make rv_check && make la_check`
- [ ] LTP 文件类（access/open/stat/rename）抽样或全量无回归
- [ ] P3 iozone 读项或 lmbench Simple stat/open 有 measurable 改善（日志或 score）
- [ ] dcache 失效用例有测试或注释说明不变量

## 风险

- F-3 **中**：错误失效导致 ENOENT/陈旧 inode → 必须跑 rename/unlink/mount 相关 LTP

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave2-fs-read-path.md

请先只做 A+B：ext4 dcache + 去掉双 path walk + 读分片放大到 4KiB。
范围最小化，make rv_check && la_check，跑 P3 iozone 或 lmbench stat 对比。
```

```
@docs/tasks/perf/wave2-fs-read-path.md

请只做 C+D：页缓存 O(1) LRU 与 install_page 去 alloc/同步预取优化。
```
