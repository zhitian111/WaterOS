# 任务 03 简报：正 dentry cache 有界 clock 淘汰

## 完成状态

已完成。another-ext4 正 dentry cache 达到 4096 项后不再 `clear()`，改为固定容量
second-chance clock，每次容量压力只淘汰一个冷条目。

## 提交

本简报与 `[perf] another-ext4 正 dentry cache 改为 clock 淘汰` 实现位于同一提交。

## 关键文件与行为

- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/positive_dentry_cache.rs`
  - 路径 `BTreeMap` 保存 inode、slot 与 reference bit，固定 slot 数组维护 clock 顺序。
  - 命中只设置 reference bit；容量压力逐项 second chance，不维护全局 LRU 链表。
  - remove/rename subtree 同步清理索引和 slot。
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/filesystem.rs`
  - lookup、insert、rename 和失效入口切换到新缓存。
- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/lib.rs`
  - 保留 `positive_clear` 兼容指标并新增 `positive_evict`。

## 验证

通过：

```bash
cd os
cargo test --offline --manifest-path \
  components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml positive_dentry_cache
cargo test --offline --manifest-path \
  components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml lookup_cache
make rv_check
make la_check
make kernel-rv-final
cd ..
git diff --check
```

定向结果为 4 个 clock/capacity 测试和 2 个既有 lookup cache 失效测试全部通过。全 crate
测试共 10 项时，9 项通过；既有 `stable_node_refcount_closes_exactly_once` 在未挂载后端的
host 环境返回 `NotMounted`，与本次缓存改动无关。

## 性能与剩余风险

任务 00/01 尚未实现，未执行 QEMU BuildStorm A/B，也未采集运行期 `positive_clear=0` 与
`positive_evict` 数据。clock 首次遇到全部 reference bit 置位时最多扫描两轮容量；这是低频
容量压力成本，替代原先丢弃全部 4096 项导致的后续缓存雪崩。若运行期出现 rename/unlink 后
陈旧 inode 或异常 ENOENT，应回退本提交。

## 文档同步

缓存策略属于内部实现；除本任务简报外无需同步公开文档。
