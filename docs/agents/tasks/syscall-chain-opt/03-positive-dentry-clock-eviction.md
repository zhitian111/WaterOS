# 任务 03：正 dentry cache 使用有界 clock 淘汰

## 任务内容与目标

替换 another-ext4 正 lookup cache 达到 4096 项后的整表 `clear()`，避免大工作集周期性缓存
雪崩。保持 rename/unlink/rmdir/mount 的精确失效语义，不把负 dentry cache 混入本提交。

## 实施方案

1. 实现固定容量的 clock、近似 LRU 或分代批量淘汰；命中不得维护高成本全局链表。
2. key 仍是规范化路径，value 保留 inode；容量和淘汰批次集中定义。
3. `remove_exact`、`remove_subtree`、rename 两端失效和 mount 清空行为保持一致。
4. 增加超过 4096 个唯一路径、热集合保留、删除后不返回陈旧 inode 的测试。

## 涉及文件

- `os/components/wateros-fs/fs-impl/impl-another-ext4/src/filesystem.rs`
- 可新增 `positive_dentry_cache.rs`
- `operations.rs` 中所有失效调用点及 crate 测试

## CodeGraph 查询

```bash
codegraph explore "AnotherExt4Fs cache_insert lookup_cache cache_remove_subtree"
codegraph callers "cache_insert"
codegraph impact "cache_remove_subtree"
```

## 验收方式

```bash
cd os
cargo test --offline --manifest-path components/wateros-fs/fs-impl/impl-another-ext4/Cargo.toml
make rv_check && make la_check && make kernel-rv-final
cd .. && git diff --check
```

诊断中的 positive-clear 必须归零，淘汰计数随容量压力增长；BuildStorm A/B 不得出现路径
陈旧、ENOENT 异常或性能回退。收益若在噪声内仍可因消除确定性雪崩合入，但简报需如实说明。

## Commit 与简报

提交建议：`[perf] another-ext4 正 dentry cache 改为 clock 淘汰`。新增
`history/03-brief.md`。
