# K-05：FS/VFS 路径解析、缓存与 I/O 放大

## 任务目标

在 K-04 证明文件系统是 Top 3 瓶颈后，降低 another-ext4 路径解析、page cache LRU、
预取和块 I/O 成本，使 iozone 及 lmbench stat/open/read 明显改善，同时保持写回与
目录操作正确。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-fs.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/features/wateros-driver.md`
- `docs/todo/perf-fs-vfs.md`
- `docs/todo/perf-risk-assessment.md`
- `docs/tasks/perf/wave2-fs-read-path.md`

## 已知信息与代码证据

以下能力已经存在，先测量而不是重做：

- RV/LA feature 都启用 `driver/impl-block-cache`；
- `BLOCK_CACHE_CAPACITY_BLOCKS` 当前为 1024；
- 块缓存支持 write-through/write-allocate；
- page cache 已预分配 frame 并批量 flush。

仍能从当前代码确认两个成本：

```rust
fn touch_lru(&mut self, idx: usize) {
    if let Some(p) = self.lru.iter().position(|&x| x == idx) {
        self.lru.remove(p);
    }
    self.lru.push_back(idx);
}
```

该 page-cache hit 为 O(capacity)。`impl-another-ext4::read_range()` 仍每次从路径
lookup inode：

```rust
let fs = self.get()?;
let inode = lookup(fs, path)?;
let attr = fs.getattr(inode)?;
fs.read(inode, offset as usize, buf)
```

这可能放大顺序读、stat/open 和 page miss，但必须由 K-04 的 hit/miss/path lookup
计数确认。

## 涉及文件

- `os/components/wateros-base/base-config/src/fs.rs`
- `os/components/wateros-driver/driver-block/block-impl/impl-block-cache/`
- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/registry.rs`
- `docs/todo/perf-fs-vfs.md`
- `docs/tasks/perf/wave2-fs-read-path.md`

## 可并行任务

- [`K-05A：稳定 inode 与 dcache`](./05a-inode-dentry-cache.md)
- [`K-05B：page-cache O(1) LRU`](./05b-page-cache-lru.md)
- [`K-05C：I/O 合并与预取`](./05c-io-merge-prefetch.md)
- [`K-05D：ramfs 物理页后端`](./05d-ramfs-physical-pages.md)

K-05A 的 file identity/cache key 契约先提交；其后 K-05B 与 K-05C 可独立实现和测量。
K-05D 是 `/tmp` 耗尽内核堆的正确性修复，不依赖 K-04 证明 FS 是性能 Top 3；
它可与 A/B/C 并行，但不得同时修改未冻结的 MM 页所有权 API。

## 任务内容

以下子项按独立提交执行；A/B 数据允许它们并行，但公共 inode/cache key 契约要先冻结：

1. **inode/dentry cache**：优先让打开的文件句柄持稳定 inode/object ID，避免每次
   range I/O 重走路径。若增加 `(mount_gen, path) -> inode` cache，必须覆盖
   rename、unlink、link、truncate、mount/unmount 和 inode reuse 失效。
2. **O(1) page LRU**：用槽位状态和侵入式 prev/next 或等价有界结构替代
   `VecDeque::position`。同步维护 free/index/LRU，不得复制 page data。
3. **I/O 合并**：依据 ext4 block size 和设备能力合并连续 read/write；不要在
   `api-v0` 固定 512B 或 QEMU 特性。
4. **预取**：只在顺序访问命中，设置批次上限；随机读取不得同步预取大量无用页。
5. **fd 分配**：只有 open/close profile 证明 registry 线性扫描占比高时才增加空闲
   位图，并保持 dup/close_range/rlimit 一致。
6. 每个子项只保留稳定超过噪声的收益，失败或负收益直接回退该独立提交。

inode 级扩展应进入 FS/VFS API，不允许 `impl-fs-bridge` 向下转型到 another-ext4
私有类型。cache key 至少区分 mount generation 和稳定对象身份。

## 如何验收

- [ ] 修改前后三轮 iozone 与 lmbench stat/open/read，报告中位数和波动。
- [ ] page/block cache hit、path lookup 和设备块数按预期变化。
- [ ] `make rv_check && make la_check` 通过。
- [ ] basic/busybox、BuildStorm 和 LTP access/open/stat/rename/unlink/truncate 通过。
- [ ] dcache 失效测试覆盖 rename overwrite、unlink 后 open fd、hardlink 和 inode
      reuse。
- [ ] page LRU 单元测试覆盖 touch、evict、dirty victim、free/index 一致性。
- [ ] 写压力后 overlay 通过 `e2fsck -fn`，原始镜像不变。
- [ ] 没有持 page-cache/VFS spin lock 执行 ext4 或块设备 I/O。

结果写入 `docs/tasks/known-issues/results/k05-<subtask>-YYYYMMDD.md`；不要把多个性能
子项合成一个无法消融的提交。
