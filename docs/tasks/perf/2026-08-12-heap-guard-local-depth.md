# Allocator guard per-CPU 深度本地化实验

## 证据

current-best 的 300s PC-hot 共采样 33,311,294,491 条指令。allocator 相关符号合计
3,945,795,184（11.84%），其中 alloc/dealloc 的 `with_allocator_interrupt_guard` 包装路径约
1,129,000,000 条。既有低开销 histogram 又显示正式编译期 TLSF 锁竞争仅约 2.3%，因此不能
把热点简单解释成全局锁等待。

当前 guard 已先禁止本 CPU 中断，却仍对本 CPU 独占的递归深度槽执行一次
`AtomicUsize::fetch_add` 和一次 `fetch_sub`。RISC-V 会为此生成 AMO；不同 CPU 的槽还可能落在
相邻 cache line。Linux `this_cpu` 的原则是在抢占/中断受控时直接读写当前 CPU 私有数据，避免
不需要的跨 CPU 原子同步：

- <https://docs.kernel.org/core-api/this_cpu_ops.html>
- <https://docs.kernel.org/core-api/local_ops.html>

## 与已失败实验的区别

2026-08-10 的 ALLOC-01A 同时修改了 guard 深度和 `used_estimate`，并把 dealloc 统计更新移入
TLSF 全局锁，最终 1800s 超时。复盘已确认严重退化来自扩大共享锁临界区。本实验只改 per-CPU
递归深度，不修改 TLSF 锁、分配算法、`used_estimate`、高水位、OOM 或指针检查。

完整八档 slab（910.08s）和旧小对象缓存也不在本分支复用；它们增加了 class/header/CPU 查询
和回收协议，无法隔离 guard AMO 的效果。

## 实现

1. 用 `#[repr(align(64))]` 的 `UnsafeCell<usize>` 表示每 CPU 深度，静态放入现有 `CpuLocal`。
2. 读取中断状态并关闭中断后，普通 load 检测递归；进入写 1，退出写 0。
3. 递归仍立即 panic，并先按原状态恢复中断；闭包仍禁止调度、等待或递归分配。
4. 不尝试解释各架构的中断状态位，也不在本实验跳过 disable/restore，保持 API 和恢复语义不变。

## 验证

- RV/LA `make check` 与 `make all`；确认默认别名等于 Final 且脚本正文标记仍在。
- 反汇编确认 guard 不再包含深度槽的 `amoadd`。
- 一次 current main/candidate BuildStorm A/B；首次明确改善即停止，不明确最多补一次对照。
- 只有功能完整且收益超过现有约 12.6s 的同内核运行波动，才合入 main。

## 结果与结论

普通/diagnostics 相关代码均未启用额外计数。RV/LA `make check`、`make all` 通过，且别名与
Final 产物一致。反汇编确认 guard 深度读写已由普通 `ld/sd` 完成，TLSF 锁的 `amoor` 和
`used_estimate` 的 `lr/sc` 保持不变。

| 内核 | commit / SHA-256 | elapsed_s |
| --- | --- | ---: |
| local-depth candidate | `bbe95971` / `11bfb26fbdc2137be05f9d1e5e21691f05791dc52c6097fb015f4ed982f83345` | 787.48 |
| current main | `86162c22` / `06d877cbaeb841a539d12b3aa96df47a4a46a9adaffe4bec90b4c5ee5717010d` | 783.00 |

候选慢 4.48s（0.57%），且差异完全落在同内核已知波动内，不能证明收益。按照一次 candidate
即可否定且不重复消耗整轮的规则，停止实验，不合入 main。结果文件：

- `/tmp/wateros-buildstorm-fixed/heap-guard-local-depth-a1/result.json`
- `/tmp/wateros-buildstorm-fixed/block-cache-second-hit-admission-a1/result.json`（current-main 基线）

该结果进一步说明 allocator 热点主要是调用次数与 TLSF 算法本身，而不是递归深度 AMO。后续
不继续堆叠 guard/统计微调；若重启 allocator 工作，必须采用结构上不同的单一 size-class 固定池，
并先解决旧 slab 命中率低和额外 CPU/header 维护的问题。
