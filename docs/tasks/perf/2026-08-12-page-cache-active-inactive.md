# 文件页缓存 active/inactive 回收实验

## 为什么选择这里

BuildStorm 缓存画像显示，文件页缓存共发生 4,718,592 次 lookup，命中率 54.11%；在
2,082,742 次容量淘汰中，有 1,195,727 页（57.41%）装入后从未被第二次访问。当前
WaterOS 对所有 clean 页使用同一条精确 LRU，新编译产物、依赖扫描和顺序读取会把只访问一次
的页面放到 LRU 尾部，并把 cargo、rustc、链接器及库文件的热页从头部逐出。

块缓存索引修复已把 matched main 从 798.93s 改善到 784.61s，但目标 Linux baseline 约
400s，继续做哈希表微调不足以缩小差距。下一步优先修复文件缓存对一次性扫描不抗干扰的
结构性缺陷。

## Linux 对照

Linux 文件页回收区分 inactive/active file LRU，而不是把所有页放进单一队列。官方
`folio_mark_accessed()` 语义会跟踪 referenced 状态，并在重复访问后把 inactive folio 晋升
active；回收主要扫描 inactive，active 保存已证明属于工作集的页面：

- <https://docs.kernel.org/core-api/mm-api.html#c.folio_mark_accessed>
- <https://docs.kernel.org/admin-guide/mm/multigen_lru.html>

完整 multi-gen LRU 依赖页表访问位、memcg/node 和后台回收，WaterOS 当前没有这些基础设施。
本实验移植其最小核心：新页先进入 inactive，只有缓存命中证明复用后才进入 active。

## 选择的方案

在现有 32 MiB、8,192 槽文件页缓存内增加第二条 clean LRU，不改变缓存容量、key、数据池或
dirty writeback 语义：

1. 新安装页和刚完成 writeback 的页进入 clean-inactive。
2. clean-inactive 页再次命中时晋升 clean-active；active 命中只移动到 active 尾部。
3. active 最多占总容量的一半；超过上限时把最老 active 页降回 inactive 尾部。
4. 分配槽位时依次选择 free、inactive 头、active 头、dirty 头；一次性扫描只淘汰 inactive，
   不再直接冲掉已证明复用的 active 工作集。
5. dirty 页仍保持独立 LRU，并沿用当前锁外 writeback/version 检查，不在本实验引入后台回写。

与完整 Linux 算法相比，这是固定比例的简化 2Q；它只增加一个 intrusive list 和常数次指针
更新，不增加逐页分配或树查询。

## 实施与验证

1. 定向单测覆盖新页进入 inactive、二次命中晋升、active 上限降级和扫描优先淘汰 inactive。
2. 运行 page-cache host tests、RISC-V check，并由 `make all` 验证 RV/LA Final 产物。
3. 使用镜像 SHA
   `ca5987d2791f83781762f531557f40fadd0a2ce0068fd9be58c2014465db7f58`，同 runner、同 CPU
   跑一组 candidate/main A/B；只以成功编译 `elapsed_s` 为性能依据。
4. 首组 candidate 明确优于 matched main 且功能 marker 通过即合入 main，不运行第二组；
   无收益则记录并回退代码。

## 风险与回退

- active 比例过大可能保护旧热点、压缩写入和新工作集空间；因此先固定为 50%。
- 小文件只读一次不会晋升，符合画像中的 scan-resistant 目标；若真实工作集复用间隔超过
  inactive 容量，本方案仍可能来不及晋升，需要后续 ghost/refault 机制。
- 若链表维护成本抵消 I/O 收益，完整回退 active LRU，仅保留本文与 A/B 结果。
