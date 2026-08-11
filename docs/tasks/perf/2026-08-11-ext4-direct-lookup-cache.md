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
4. 只有完整轮可复现改善至少 1.5%，且无功能失败或明显波动，才允许合并 main。
5. 未达门槛则回退全部代码，只保留本文的验证结果与原因。

## 验证结果

### 正确性与构建

- another-ext4 host tests：5 项通过，包含同槽冲突替换、remove 子树和 rename 子树。
- RISC-V Final check：通过。
- LoongArch Final check：通过。
- RISC-V Final kernel build：通过。
- 两个完整候选轮均通过 toolchain、minibuild、compile marker 和 judge；无 panic、
  SIGSEGV、stall 或 timeout。

### 匹配 pc-hot

候选与 main 都绑定 CPU `0-15`，使用同一镜像
`4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1`，运行 300 秒并
进入同一 `arceos-helloworld` 编译阶段：

| 指标 | main | direct-map |
|---|---:|---:|
| 总 guest 指令 | 29.85B | 32.60B |
| `AnotherExt4Fs::lookup` | 230.52M | 203.06M |
| `lookup` 占总指令 | 0.772% | 0.623% |
| `memcmp` | 1.519B | 0.376B |
| `dir_find_entry` | 197.10M | 225.58M |
| cache insert | 7.53M（BTreeMap） | 9.69M（direct-map） |

direct-map 确实消除了大量 BTree 路径比较，但 `dir_find_entry` 占比略升，符合固定槽冲突
增加 lookup miss 的预期。插件窗口内候选推进更多指令，不能单独替代完整轮验收。

### 完整 BuildStorm

所有有效样本均使用上述同一镜像、`-snapshot`、CPU `0-15`、`TMPDIR=/tmp`：

| 实现 | elapsed_s |
|---|---:|
| main | 810.71 |
| main | 815.06 |
| direct-map | 822.09 |
| main（夹心对照） | 835.58 |
| direct-map | 815.23 |

- direct-map 中位数：`818.66s`。
- 三个 main 样本中位数：`815.06s`。
- 相对全部 main 中位数，候选慢 `3.60s`（约 0.44%）。
- 只相对中间的 835.58s main，候选两轮中位数看似快约 2.0%，但该 main 明显慢于
  同一内核、同一镜像的前两轮，不能把宿主漂移当成实现收益。

## 结论与决策

代码不保留、不合并 main，只提交实验记录。direct-map 在指令层面减少了路径树比较，但
完整轮没有可复现达到 1.5% 的净改善；固定槽冲突还会增加真实 ext4 目录扫描。该结果应
归类为“机制成立、端到端低收益且受环境漂移影响”，不是功能失败，也不是明确性能退化。

后续不继续猜测缓存淘汰策略。先用低扰动 syscall/参数 profiler 测量 BuildStorm 的路径
复用距离、正负 lookup 比例以及一次性路径流量，再决定是否采用热点保护、流式旁路或
negative dentry cache。
