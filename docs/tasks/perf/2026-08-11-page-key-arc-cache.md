# 页缓存 FileCacheKey Arc 缓存（2026-08-11）

## 为什么选择这里

最新 pc-hot 中，页缓存 `FileCacheKey::cmp` 约 `183M`，TLSF allocate/deallocate 仍合计
约 `1.7B`。`PagedFileHandle` 的每次 read/write/seek/truncate 都会调用
`cache_file_key()`：

```text
file_key_for_state
  -> Arc::from(state.cache_key.as_str())
  -> FileCacheKey::stable/path
```

`DetachedState.cache_key` 在打开后通常不变；只有 rename 且没有 stable node 时才更新。
每次构造 `FileCacheKey` 都重新分配并复制一遍 `Arc<str>`，属于可以安全消除的重复工作。

## 优化方案

1. 在 `DetachedState` 中增加 `cache_key_arc : Arc<str>`，与 `cache_key : String` 同时
   初始化。
2. `file_key_for_state` 改为克隆缓存的 `Arc<str>`，不再执行 `Arc::from`。
3. rename 更新 `source_state.cache_key` 时同步更新 `cache_key_arc`。
4. 不做旧版 canonical-key 实验中的额外 `BTreeMap::get_key_value` 路径，也不改变
   `FileCacheKey` 比较或哈希语义。

## 为什么这么做

这是直接从热路径去掉一次字符串 Arc 分配/释放的小改动，风险低。它不是之前已否决的
“规范化 key 回传”，因为这里没有增加额外 BTree 查找或 Arc clone；每次调用从“新建
Arc”变成“clone 已缓存 Arc”，clone 只是引用计数递增。

## 下一步

1. 实现 `cache_key_arc` 并覆盖 rename 更新。
2. 双架构 Final check/build，跑 Final smoke 覆盖 rename/read/write。
3. 完整 BuildStorm A/B 与 300 秒 pc-hot A/B。
4. 有效则合并 main，无效则回退并记录。

## 验证结果

- 双架构 Final `make check` 通过。
- Final smoke 通过：根卷、VFS 自检、cagent 全部通过，并进入 BuildStorm。
- 完整 BuildStorm：
  `BUILDSTORM_COMPILE mode=multi ok=true elapsed_s=813.48`，相对当前 main
  `809.42s` 慢约 `0.50%`。
- 300 秒 pc-hot：总指令、memcpy、copy_from_user 和 FileCacheKey 相关符号均高于
  当前 main，说明新增的 `DetachedState.cache_key_arc` 字段/初始化/rename 维护没有带来
  净收益。

## 结论

完整 BuildStorm 和 pc-hot 都未显示收益，代码已全部回退，只保留本记录。页缓存 key
构造的 Arc 分配并不是当前 BuildStorm 的主要可兑现瓶颈，不应继续堆叠 key 缓存。
