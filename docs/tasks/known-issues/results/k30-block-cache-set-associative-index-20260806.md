# K-30 block cache 8 路组相联索引（2026-08-06）

## 问题

`pc-hot` 显示 `CachingBlockDevice::read_blocks` 是内核侧最大热点之一，反汇编确认
耗时的核心是 `BTreeMap<Lba, usize>` 的查找循环，而不是数据复制。该热点在完整
BuildStorm 早期阶段约占内核大量指令。

## 修改

将 block cache 的 LBA 索引从 `BTreeMap<Lba, usize>` 替换为固定容量 8 路组相联
哈希表：

- 每个 bucket 最多 8 个 `(Lba, slot_idx)`。
- lookup/update/remove 最多扫描 8 个槽，无 BTreeMap 对数查找。
- 无 tombstone、无 rehash、无开放寻址搬移；bucket 冲突时按缓存语义淘汰该 bucket
  中的旧项并回收对应 slot。
- 对外 `BlockDevice` 语义、LRU 和写穿策略不变。

## pc-hot A/B

同一 180 秒 Final 早期阶段：

| 指标 | BTreeMap 基线 | 8 路组相联 |
|---|---:|---:|
| `read_blocks` 指令 | 约 7.3B | 约 3.8B |
| `cache_put` 指令 | 约 0.60B | 不再进入 Top-15 |

## 验证

```text
cargo test --manifest-path os/components/wateros-driver/driver-block/block-impl/impl-block-cache/Cargo.toml
make rv_check
make la_check
make kernel-rv-final
make kernel-rv-pre
```

完整 Final：

```text
BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=1365.70 cores=8 bytes=1681000 arch=riscv64
#### OS COMP TEST GROUP END buildstorm-glibc ####
```

这是最近完整 Final 的最优记录，之前为 `1567-1690s`。Pre 可行性：
`sdcard-rv.img` 60 秒进入 hackbench/cyclictest，无 panic 和 ext4 读块错误。
