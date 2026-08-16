# Task 01 简报：VFS dcache 快速实验结论

## 状态

尝试了三种低复杂度 dcache/readahead 变体，均未通过单轮 LA BuildStorm 性能门槛，
已回退，工作树保持干净。

## 实验数据

```text
frame 基线 LA:                538.64
positive-only 全局 metadata cache: 547.10
positive-only 16-shard cache:     559.28
readahead stride 8 -> 16:         564.40
```

## 结论

- path-string 元数据缓存会带来额外锁、克隆和失效开销；
- 全局锁和简单 shard 都没有抵消这些开销；
- 当前 ext4/VFS 查找并非直接加一个内存缓存就能稳定受益的瓶颈。

## 下一步

先做低开销 VFS 诊断，量化 `statx/openat/metadata` 的查找成本和重复率；若 ext4
查找不是主成本，则转向 fork/exec 短进程或 futex/clock 路径。

