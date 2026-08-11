# another-ext4 正路径组相联缓存实验

## 为什么选择这里

current-best 的 300 秒 `pc-hot` 显示，内核 `memcmp` 共约 1.735B 条指令；按动态调用点
回溯，`AnotherExt4Fs::lookup` 单个调用点执行了 8,634,813 次 `memcmp`，远高于其他内核
调用点。当前正路径缓存是容量 4,096 的 `BTreeMap<String, u32>`，一次命中需要多次路径
比较，满表还会整体清空。

历史单路 direct-map 已证明哈希索引能把 `memcmp` 从 1.519B 降到 0.376B，但单槽冲突使
`dir_find_entry` 从 197.10M 升到 225.58M，完整 BuildStorm 中位数反而慢约 0.44%。因此
本实验不重复单路表，也不改写通用 `memcmp`；后者的 64 字节展开曾确定性退化 22.18%。

## 候选方案

将正路径缓存改为与已验证有效的 negative dentry cache 同型的固定容量 4-way
set-associative 表：

1. 仍保存完整规范化路径、FNV-1a hash 和 inode；命中同时校验 hash 与完整路径。
2. 总容量仍为 4,096，分为 1,024 个 bucket，每个 bucket 四路；优先使用空槽，满组后
   round-robin 淘汰，不维护全局 FIFO/LRU。
3. 命中只计算一次路径 hash，最多比较四个 hash，通常只做一次完整路径比较。
4. mount、create、unlink/rmdir、rename 的发布和子树失效语义保持不变；冲突只导致 miss，
   不会产生错误 inode。
5. 不修改 vendor、块缓存、negative cache 或通用内存原语，以便把 A/B 归因到正缓存索引。

相对单路 direct-map，四路组相联直接针对已观测到的冲突回退；相对 `BTreeMap + FIFO`，它
没有每次插入维护队列的成本，也不在容量边界整体清空热点集合。

## 验证与保留门槛

1. 定向测试覆盖命中/更新、同 bucket 四路共存、第五项局部淘汰、子树删除和 rename。
2. another-ext4 host tests、RISC-V/LoongArch Final check、`make all` 通过；只在失败时读取
   构建日志。
3. 先做短启动回归；随后使用 current-best 的同一镜像和 runner 口径跑一次完整 candidate。
4. 相对最近 current-best 783.00s 有明确改善即按一次有效 A/B 接受；若结果落在已知噪声
   范围或退化，不追加多轮，回退代码并只保留实验记录。
5. 接受后合入 main，并重新确认 `make all` 默认产物和 `SCRIPT_BODY_FLAT_BEGIN` 标记。

## 实现与结果

实现了总容量 4,096、4-way/1,024 bucket 的正路径缓存，完整路径校验、局部
round-robin 淘汰以及 mount/rename/remove 失效语义均保留。定向测试新增同 bucket 四项
共存和第五项局部淘汰覆盖。

验证结果：

- another-ext4 host tests：通过；
- RISC-V/LoongArch Final check：通过；
- `make all`：通过，RV/LA 默认产物均等于 Final，且包含 `SCRIPT_BODY_FLAT_BEGIN`；
- candidate 内核 SHA-256：
  `2fa6dd1017c09f88fa915a45a73236e8ee37d03c8a53f128f01ae011fcb2dd49`；
- 镜像 SHA-256：
  `ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`；
- 完整 BuildStorm：全部 marker/judge 通过，无 timeout、stall、panic 或 SIGSEGV，正式编译
  `783.57s`，runner 总墙钟 `813.804s`。

最近 current-best 同口径为 `783.00s`。候选慢 `0.57s`（约 0.07%），远小于已观测的
约 12.6s 同内核波动范围。按一次有效 A/B 规则判定为无可验收收益，不追加第二轮、不合入
main。该结果也再次说明降低通用 `memcmp`/路径比较指令数不能直接推导出 BuildStorm 墙钟
收益；下一步应优先减少跨层复制或更高层调用次数。

结果文件：

- `/tmp/wateros-buildstorm-fixed/ext4-positive-set-assoc-a1/result.json`
