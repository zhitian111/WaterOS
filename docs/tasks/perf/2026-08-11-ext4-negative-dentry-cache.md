# ext4 negative dentry 与元数据查找缓存

## 为什么选择这里

固定 BuildStorm 300 秒 syscall 画像中：

| syscall | 次数 | 路径重复率 |
| --- | ---: | ---: |
| `statx` | 58,881 | 81.99% |
| `openat` | 24,025 | 69.90% |
| `readlinkat` | 10,709 | 92.14% |

当前 main 已有两层正缓存：

- another_ext4 内部 4,096 槽 inode snapshot cache，避免重复 inode 块解码和拥有副本分配；
- `AnotherExt4Fs` 中容量 4,096 的完整路径 `BTreeMap<String, u32>` 正 lookup cache。

现有缓存不记录 `NotFound`。Cargo/rustc 会反复探测可选配置、候选库名、增量产物和父目录；
同一个不存在路径每次都会重新执行 `generic_lookup -> dir_find_entry`。当前 300 秒 pc-hot 中
`AnotherExt4Fs::lookup` 与 `dir_find_entry` 合计约占 guest 指令 1.5%，并可能继续下探 ext4
块缓存和 VirtIO 忙等。

历史实验已经否定：

- 给正缓存增加 FIFO 队列：完整轮退化约 3.8%；
- 把正缓存改成单路 direct-map：memcmp 显著下降，但冲突增加真实目录扫描，完整轮无净收益；
- 单纯扩大 another_ext4 内层块缓存到 4 MiB：完整轮退化约 1.6%。

本项不重复这些方案，而是补齐独立 negative cache；负项冲突或淘汰只损失一次优化命中，
不会驱逐现有正项，也不会改变文件系统真相。

## 先做的画像

增加只在 profiling feature 中存在的低扰动累计计数：

- 正缓存 hit；
- 正缓存 miss 后 lookup 成功；
- lookup 返回 `NotFound`；
- negative cache hit；
- 正缓存达到容量并整表 clear 的次数；
- create/rename 等写路径触发的负项失效次数。

计数不逐次打印，只在有界间隔设置 pending 标志，由 syscall 返回后的安全点输出累计快照。
诊断运行不作为墙钟成绩；分析时只读取计数行和 `result.json`。

## 候选设计

### 1. 独立固定容量 negative cache

在 `AnotherExt4Fs` 中增加与正缓存分离的固定容量缓存：

- key 为规范化完整路径的 64-bit hash，并保存完整路径做碰撞校验；
- value 只表达该路径在当前挂载实例中已确认 `NotFound`；
- 先查现有正缓存；正 miss 后查 negative；负命中直接返回 `FsError::NotFound`；
- 负 miss 才执行真实 `generic_lookup`，成功发布正项，`NotFound` 发布负项；
- 固定槽冲突只覆盖另一个负项，不会产生错误命中或损失正缓存命中。

容量和组相联度由画像决定；第一候选优先使用小型 4-way/8-way set-associative 表，避免
单路 direct-map 的冲突问题，又不维护 FIFO/LRU 队列。

### 2. 精确失效

- create/write-create/mkdir/mknod/hardlink：移除目标路径负项；
- rename：移除目标路径及其子树负项，避免目录树移入后看到陈旧 `NotFound`；
- mount/remount：清空全部负项；
- unlink/rmdir：负项仍表达不存在，不需要为正确性清空；正缓存继续沿用现有子树失效；
- 哈希命中必须再比较完整路径，碰撞只能变成 miss，不能返回错误 `NotFound`。

### 3. 后续扩展边界

若 full-path negative hit 率高但完整收益仍受路径处理限制，再单独画像并设计
`(parent inode, basename)` component dentry cache。当前实验不修改 vendored `generic_lookup`
或同时替换正缓存，避免混合变量。

## 实施与验证步骤

1. 提交本文后再修改代码。
2. 增加 profiling feature 与定向缓存/失效测试，运行 300 秒诊断。
3. 只有重复 `NotFound` 足以覆盖缓存查询成本时，启用普通 Final 候选。
4. 运行相关 host tests、RISC-V/LoongArch Final check 与 RISC-V Final build；日志只在失败时读取。
5. 180 秒 smoke 验证 create/unlink/rename 与 BuildStorm 启动无语义回归。
6. 使用固定镜像、CPU `0-15`、`TMPDIR=/tmp`、QEMU `-snapshot` 做 main/candidate 完整 A/B。
7. matched pc-hot 检查 `lookup/dir_find_entry`、VirtIO、memcmp、TLSF 以及新缓存本身。

## 验收与回退

- 所有命中必须通过完整路径校验；create/rename 后不得返回陈旧 `NotFound`；
- 双架构 check、定向测试和完整 BuildStorm 全部通过；
- 相对同哈希 main 有稳定正收益，目标至少 1.5%；最终是否合并以完整轮和用户判断为准；
- 若 negative hit 率低、缓存本身成为热点或完整轮无收益，回退实现，仅保留画像与结果文档。

## 实现与结果

### 实现

- 新增 4-way set-associative、总容量 4,096 的独立 negative cache；槽保存 FNV-1a hash
  和完整路径，碰撞必须再比较路径。
- 正缓存命中路径完全不进入负缓存；正 miss 才检查负项，真实 `NotFound` 才发布负项。
- 正项发布移除同路径负项；rename 清除目标子树负项；mount 清空；内部 orphan 目录/链接
  创建也同步发布正项，避免隐藏路径留下陈旧负项。
- `lookup-diagnostics` 通过 another-ext4 → fs aggregate → kernel root feature 显式转发；普通
  Final 编译已确认不包含 `BUILDSTORM_FS_META_COUNTERS` 字符串。
- 定向测试覆盖同 bucket 碰撞的完整路径校验、exact 失效、子树边界、rename 目标负项失效
  与正项发布失效。

### 300 秒画像

固定镜像 SHA-256：
`4e6d6536096178b88cfab801743f1f634fb3755b3af5ca69bb998e798fba57f1`；诊断内核
SHA-256：`a9cfe80d9f406dbddba28ba9e4c072b96a1f84116eabfd489d1bcb3d5c2ef3b2`。
运行通过 toolchain/minibuild 并进入正式编译。最后一个累计快照：

| 计数 | 数值 | 占全部 lookup |
| --- | ---: | ---: |
| total | 1,048,576 | 100% |
| positive hit | 1,003,250 | 95.68% |
| lookup success after miss | 23,723 | 2.26% |
| real ext4 `NotFound` | 6,231 | 0.59% |
| negative hit | 15,372 | 1.47% |
| positive full clear | 5 | — |
| invalidated negative entries | 490 | — |

在全部不存在结果中，negative hit 率为
`15,372 / (15,372 + 6,231) = 71.16%`。约七成重复不存在路径已绕过真实 ext4 查找，
达到进入普通 Final 完整 A/B 的条件。

### 验证进度

- another-ext4 普通与 diagnostic host tests：通过。
- RISC-V/LoongArch Final check：通过。
- RISC-V diagnostic Final check/build：通过。
- 普通 RISC-V Final build：通过，候选 SHA-256：
  `13c7a69800f930d1ce6ab74575480fa13328319676d715165c8624e26edba7a0`。
- 180 秒 smoke：通过 toolchain/minibuild，进入正式编译；无 panic、SIGSEGV 或 stall。
- current main（含已合并 ELF 共享页）基线内核 SHA-256：
  `4e09fc3f2dadc3af45fc0d7ba7cb74bbf8db9b9eac5b0b97f554dac6f6bdd86e`。

### 完整 A/B 与合入决定

固定条件：同一 BuildStorm 镜像、CPU `0-15`、`TMPDIR=/tmp`、QEMU snapshot；两次均通过
toolchain、minibuild 和正式 compile 判定，无 panic、SIGSEGV 或 stall。

| 顺序 | 内核 | guest compile | host wall | SHA-256 |
| --- | --- | ---: | ---: | --- |
| A1 | negative dentry candidate | 787.76s | 818.726s | `13c7a69800f930d1ce6ab74575480fa13328319676d715165c8624e26edba7a0` |
| B1 | current main（含 ELF 共享页） | 807.11s | 837.776s | `4e09fc3f2dadc3af45fc0d7ba7cb74bbf8db9b9eac5b0b97f554dac6f6bdd86e` |

同一交错对照中候选快 `19.35s`，即 `2.40%`。A2 已以同一候选内核启动，但因
LoongArch BuildStorm 出现 `TCG: unaligned access support required`、导致该架构直接无分，
按更高优先级主动停止，避免继续占用约十分钟测试时间。结合 71.16% 的重复 `NotFound`
命中率、完整 A1/B1 的正收益以及用户明确确认，本项纳入 main；未把中止的 A2 当作性能
样本，也不宣称已经取得多轮统计稳定性。
