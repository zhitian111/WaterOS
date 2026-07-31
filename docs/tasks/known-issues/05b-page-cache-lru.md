# K-05B：Page cache O(1) LRU 与槽位不变量

## 任务目标

在 K-04 证明 page-cache lock/LRU 是瓶颈后，将 hit/evict 的 O(capacity) 操作改为
O(1)，保持 dirty version、free/index/LRU 和并发 writeback 一致。

## 执行前必读

- `docs/tasks/known-issues/05-fs-vfs-performance.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/todo/perf-fs-vfs.md`

## 已知信息与代码证据

当前每次 touch 在线性搜索 `VecDeque`：

```rust
if let Some(p) = self.lru.iter().position(|&x| x == idx) {
    self.lru.remove(p);
}
self.lru.push_back(idx);
```

frame 已按 capacity 预分配，因此可用 slot index 维护 prev/next，无需移动 page data。

## 当前进度（2026-07-31）

已用 clean/dirty 两条侵入式双向 LRU 替换 `VecDeque` 线性搜索。hit、class
迁移、remove 和 clean-first victim 选择均为 O(1)，dirty writeback 仍保持锁外执行。
host 9 项测试和双架构 check 通过；fresh final 在相同 21/446 观察点由约
10分21秒缩短到约 8分27秒。完整 BuildStorm、三轮性能基准和测试后 e2fsck
仍属于总体验收，详见
[`results/k05b-20260731.md`](./results/k05b-20260731.md)。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`
- `os/components/wateros-base/base-config/src/fs.rs`
- `docs/todo/perf-fs-vfs.md`

## 任务内容

1. 为每个 slot 增加 LRU linked state 或等价 O(1) 结构。
2. 统一实现 insert/touch/remove/pop；free、index、key 和 LRU membership 必须同步。
3. dirty victim 仍走现有 snapshot/version 锁外 writeback，不能在 state 锁内 I/O。
4. 增加 debug invariant checker，仅测试/诊断构建启用。
5. 不在同一提交调整 cache 容量、预取和 ext4 I/O，保持可消融。

## 如何验收

- [ ] 单元测试覆盖重复 touch、clean/dirty evict、free reuse、flush race 和 purge。
- [ ] invariant 检查无 duplicate/missing slot，测试结束 active 状态回到基线。
- [ ] iozone re-read/page hit 锁耗时有稳定改善。
- [ ] BuildStorm、FS LTP、`e2fsck -fn` 和双架构 check 通过。

交付 `docs/tasks/known-issues/results/k05b-YYYYMMDD.md`。
