# runtime-heap-allocator

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

本 crate 提供 WaterOS 的 `#[global_allocator]`。默认 backend 为 TLSF，可用
`impl-linked-list-allocator` 切回 `LockedHeap`；二者互斥。它管理固定静态内核堆，不管理 guest
物理内存、用户页、page cache 或 swap。

## 代码地图

| 文件 | 职责 |
| --- | --- |
| `src/lib.rs` | `HeapMemStats`、静态 `HEAP_SPACE`、init、OOM handler 和后端选择 |
| `src/backend_tlsf.rs` | TLSF pool、used 估算、指针范围检查和 GlobalAlloc |
| `src/backend_linked_list.rs` | `LockedHeap` 回退后端和精确 used/free |
| `src/interrupt_guard.rs` | per-CPU 关中断、递归分配检测和 90% 高水位告警 |
| `src/stress.rs` | 多 size-class 非 LIFO 碎片压力，仅专用 feature |

堆容量由 `wateros-base/base-config/src/mm.rs::KERNEL_HEAP_SIZE` 决定，当前是 `1 << 29`，即
512 MiB。`HEAP_SPACE` 位于链接段 `.kernel.heap`；修改容量要同时检查链接布局、物理内存范围、
TLSF 位图参数和两架构镜像大小，不能只改数组常量。

## 初始化与同步

只有 BSP 在单线程启动阶段调用一次 `init()`；AP 必须在发布屏障之后才进入会分配的路径。
重复 init 会把正在使用的 allocator metadata 当空池重建，属于内存破坏，不是合法重试。

```text
GlobalAlloc::{alloc,dealloc,realloc}
  -> 读取当前 CPU 中断状态
  -> 关闭本 CPU 全局中断
  -> CpuLocal HEAP_GUARD_DEPTH += 1
       -> 已非 0: 恢复状态并 panic（递归分配）
  -> backend Mutex/LockedHeap lock（跨 CPU 互斥）
  -> 分配器操作
  -> depth -= 1
  -> 恢复进入前中断状态
```

关本 CPU 中断不能替代跨 CPU 锁；backend 锁也不能阻止同一 CPU 中断重入后自旋，所以两层都需要。
guard 闭包内不得调度、等待、执行 VFS 回调或产生会再次分配的日志格式化。

当前 guard 不是 unwind RAII 对象：闭包若 panic，不会执行后面的 depth 减一/中断恢复，但内核 panic
随后终止系统，因此普通执行不依赖恢复。不要把 allocator 内部 panic 改成可捕获继续运行。

## TLSF 与 linked-list

| 项目 | TLSF | linked-list |
| --- | --- | --- |
| 选择 | 默认 `impl-tlsf` | `HEAP_ALLOCATOR_FEATURE=heap-linked-list` |
| 目标 | O(1) size-class 查找 | 简单回退与 A/B 诊断 |
| stats used | 按请求 layout size 的原子估算 | allocator 报告值 |
| 碎片 | 低且有界查找，但仍可能没有足够连续块 | 随 churn 可能增加查找/碎片成本 |
| 非法 dealloc | 范围/对齐检查；默认告警一次并忽略 | 交给后端契约 |

TLSF 的范围检查只能证明指针和 `ptr+size` 落在 pool 且满足 layout alignment，不能证明它是 allocation
起点，也不能检测 double-free。启用 `tlsf-diagnostics` 后非法指针直接 panic，适合最小复现构建；
普通构建只告警一次是为了避免损坏路径递归刷日志，但忽略并不表示安全。

## realloc 不变量

- `ptr == NULL` 等价于按新 layout 分配；
- `new_size == 0` 释放旧块并返回 NULL；
- layout 构造失败返回 NULL；
- 成功后 used 估算先减旧 size、再加新 size；
- 失败时旧 allocation 仍由调用者拥有，used 不变。

任何自定义容器调用 GlobalAlloc 都必须用与分配时相同的 size/alignment 释放。layout 不配对会让统计失真，
更严重时破坏 allocator metadata。

## stats 的正确解释

`heap_mem_stats()` 是瞬时诊断值，不可用于回收决策。

```text
capacity = 静态池编译容量
TLSF used = 成功 alloc layout.size 累加 - dealloc layout.size
TLSF free = pool_len - used_estimate（饱和）
```

它不精确计入 alignment padding、TLSF metadata 和外部碎片。因此：

- `free >= request` 仍可能因没有足够大的连续 free block 而 OOM；
- used 下降不证明物理用户页已回收；
- `/proc/meminfo MemFree` 很高不证明固定内核堆有空间；
- QEMU `-m` 从 1G 加到 8G 不会自动扩大编译期 `HEAP_SPACE`。

## OOM 排查

OOM handler 打印：`layout_size`、`align`、`used`、`free`、`cap`，然后 panic。它不能在失败后尝试分配
诊断 Vec/String，也不能“返回 ENOMEM”给未知 Rust 分配调用者；可恢复用户请求应在更上层使用
`try_reserve`/可失败 buffer，避免触发 global alloc handler。

判断顺序：

1. `layout_size` 是否来自用户可控长度或 Vec 几何扩容；
2. 该对象应否使用 page/frame allocator，而非固定 heap；
3. fork/clone 是否深复制本应 Arc 共享的表或 buffer；
4. exit-time 是否释放 fd/pipe/waiter，reap-time 是否释放侧表/aspace；
5. live object 数回落后 used 是否仍上升；
6. TLSF 与 linked-list A/B 是否都在相同业务点失败；
7. 小块总量稳定但大块失败，是否属于碎片。

典型 1 MiB layout 往往是 Vec capacity 扩容，不等于某个业务对象正好 1 MiB。用 backtrace/临时调用点计数
找到增长容器，不要只按 layout 大小搜常量。

## 碎片与泄漏区分

| 现象 | 更可能 |
| --- | --- |
| live 对象与 used 一起单调上升 | 所有权/清理泄漏 |
| live 对象回落、used 估算不回落 | layout 不配对或隐藏侧表 |
| used/free 看似足够但大块失败 | 外部碎片 |
| TLSF 正常、linked-list 长时失败/变慢 | 后端碎片/查找差异 |
| 两后端在同轮同对象数 OOM | 上层真实容量需求/泄漏 |
| 第一轮增长、后续平台期稳定 | 缓存/高水位，不足以判泄漏 |

启用顶层 `heap-stress` 会在 init 后运行固定多尺寸 churn、打印 early/late raw 平均和 stats，最后永久挂起。
它只验证 allocator 模式，不创建真实 fork/fd/VMA 生命周期，不能代替 `stress-ng --forkheavy`。

## 回归方法

先做后端自身：

```sh
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
make check ARCH=rv PROFILE=pre HEAP_ALLOCATOR_FEATURE=heap-linked-list
```

再在同一新启动 guest 连跑至少两轮 workload，记录每轮前后 heap、task/zombie、aspace/VMA、fd/OFD/pipe、
futex/signal/SHM 表项。成功标准不是“未在 60 秒内 OOM”，而是 workload 成功且第二轮资源回到稳定区间。

修改 allocator 本身还要覆盖：0/小/大尺寸、各 alignment、realloc grow/shrink/fail、跨 CPU churn、非法指针
diagnostic 构建、接近容量和释放后大块再分配。普通 pre/final 构建不得启用 `stress-on-init`。
