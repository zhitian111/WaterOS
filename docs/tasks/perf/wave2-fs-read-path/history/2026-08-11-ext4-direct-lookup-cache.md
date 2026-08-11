# another-ext4 direct-mapped 路径查找缓存实验

## 为什么选择这里

固定口径 300 秒 `pc-hot` 中，`AnotherExt4Fs::lookup` 与
`another_ext4::Ext4::dir_find_entry` 仍分别消耗约 2.47 亿、2.05 亿条指令。
当前 `AnotherExt4Fs` 已有容量 4096 的 `BTreeMap<String, u32>` 正向路径缓存，
但每次命中需要树查找和多次字符串比较；插入第 4097 个不同路径时还会清空整表。

历史 `perf/vfs-lookup-fifo` 已把满表清空改成 `BTreeMap + VecDeque` FIFO，完整轮
`907.81s` 对同轮 main `874.46s`，退化 3.8%。该方案的队列维护成本已经被否定，本实验
不重复 FIFO/LRU，也不修改 vendor 的 `dir_find_entry`；后者的原地解析实验曾退化 1.66%。

## 选择的方案

把路径缓存改成 4096 槽的 direct-mapped 表：

1. 对完整规范化路径计算轻量 64-bit FNV-1a，低位选择一个固定槽。
2. 槽内保存 hash、拥有型路径和 inode；命中必须同时满足 hash 与完整路径相等，哈希冲突
   只会产生 miss，不影响正确性。
3. miss 成功后直接替换对应槽，不维护 FIFO/LRU，不存在满表整体清空。
4. mount 清空全部槽；unlink/rmdir 按路径前缀扫描并清除；rename 扫描槽并迁移源子树，
   同时清除目标子树，保持当前缓存失效语义。
5. 缓存仍只保存正向 lookup，不引入 negative dentry，也不成为文件系统真相源。

## 为什么这样做

- 命中从 `O(log n)` 次路径比较变成一次线性哈希和最多一次完整比较。
- 固定槽替换不需要队列更新，直接针对 FIFO 实验暴露的额外维护成本。
- 4096 个槽维持现有内存上界；冲突只降低命中率，不引入陈旧 inode。
- 改动限制在 another-ext4 adapter，现有 create/unlink/rename 语义可由定向单测覆盖。

## 验证与保留门槛

1. 单元测试覆盖命中、同槽冲突替换、remove 子树和 rename 子树。
2. RISC-V/LoongArch Final check 与受影响内核构建通过。
3. 固定 P 核 `0-15`、同一镜像、`TMPDIR=/tmp`、`-snapshot` 跑 `pc-hot` 和完整
   BuildStorm A/B。
4. 以当前 main 固定口径中位数 `812.885s` 为初始对照；只有完整轮可复现改善至少
   1.5%，且无功能失败或明显波动，才允许合并 main。
5. 未达门槛则回退全部代码，只保留本文的验证结果与失败原因。

