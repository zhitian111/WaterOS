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

