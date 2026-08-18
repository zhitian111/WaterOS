# 栈式物理帧分配器实现

[组件手册](../../../README.md) · [API](../../frame-alloctor-api/api-v0/README.md)

`StackFrameAllocator` 管理一个 4 KiB PPN 半开区间和至多一个保留子区间。新帧从高地址向低地址取，归零引用的帧进入 LIFO 回收栈。全局实例由 BSP 初始化，运行期以“关本 CPU 中断 + SMP 自旋锁”保护。

## 核心字段与不变量

| 字段 | 含义 | 不变量 |
| --- | --- | --- |
| `start_ppn..end_ppn` | 管理的完整半开区间 | 初始化后不变。 |
| `reserved_start_ppn..reserved_end_ppn` | 区间内永久排除部分 | novel、回收和引用接口均不能返回/接受。 |
| `next_novel` | 尚未触碰连续段的上界 | 从高向低递减，遇保留区整段跳过。 |
| `recycled` | 引用归零的 LIFO 帧 | 不应包含 allocated、越界或保留帧。 |
| `allocated` | 每帧是否在用 | 与 `ref_counts != 0` 保持一致。 |
| `ref_counts` | PTE/cache 等共享引用数 | 归零时才压入 recycled，溢出必须报错。 |

注意最后一行是目标不变量，不完全是实现现状：`inc_ref` 当前使用 `u32::saturating_add(1)`，达到 `u32::MAX` 后静默饱和而不是报错。极端情况下随后 dealloc 永远无法归零。应改为 `checked_add` 并返回明确 overflow 错误，至少返回 `InvalidFrame`，且加入定向测试。

## 初始化与元数据成本

`init_with_reserved(start,end,res_start,res_end)` 会清空 recycled，将保留区 clamp 到主区间，设置 `next_novel=end`，并把 `allocated`/`ref_counts` resize 到主区间总页数。它不会验证 `start <= end`；反序区间因 saturating_sub 得到零元数据，但其它边界字段仍不形成合法池，调用者必须先拒绝。

虽然 novel free-list 不为每个空闲 PPN 存一个 `usize`，实现仍为每个物理页分配：

- `allocated: Vec<bool>`，约 1 bit/page；
- `ref_counts: Vec<u32>`，4 bytes/page；
- 已回收但尚未重用的 `recycled: Vec<PhysPageNum>`，最坏 8 bytes/page（64 位）。

8 GiB RAM 约 2,097,152 页，仅 refcount+bitmap 就约 8.25 MiB 内核 heap；若 recycled 容纳大量离散空闲页，capacity 最坏再接近 16 MiB。`/proc/meminfo` 物理页很多时，初始化或长期 churn 仍可能把固定 512 MiB kernel heap 推高。诊断必须同时观察 heap used、`recycled.len/capacity` 与 frame stats。

全局 `READY` 在静态 cell 写入后 Release=true，随后才在锁内调用 allocator init。源码契约要求 BSP 在开放 AP 前完成整个函数；READY 本身不能让并发首次初始化安全，也不能阻止另一个 CPU 在“ready 但尚未 init”窗口分配。不要在运行期重新初始化生产 allocator；自测 reset 仅适用于没有其它 owner/waiter 的阶段。

## 分配与引用状态机

```text
从 recycled.pop
  -> 丢弃越界/保留/仍 allocated 的坏项并告警
  -> allocated=true, ref=1
否则 next_novel 向下移动（到保留区上界时整段跳到下界）
  -> allocated=true, ref=1
inc_ref -> ref += 1
dealloc(ref>1) -> ref -= 1
dealloc(ref==1) -> ref=0, allocated=false, recycled.push
```

dealloc 拒绝越界、保留区、尚未从 novel 取出的低 PPN、allocated=false 或 ref=0。重复释放不会重新入栈。`frame_dealloc()` 兼容接口会记录后忽略错误；需要维护所有权不变量的代码必须用 `frame_dealloc_result()` 并处理失败。

`recycled.push` 使用不可失败 Vec 增长；归还最后一个物理引用本身可能触发 kernel heap allocation error。更稳的设计是让 free-list 链指针存入空闲物理页本身，或使用预留元数据/分段 bitmap，避免“为了释放 RAM 还要分配 heap”。

## 锁与中断规则

`with_frame_allocator` 保存中断状态、关闭本 CPU 全局中断、获取 `MultiprocessorSafeCell`，闭包结束后先放锁再恢复原状态。这样阻止同 CPU 中断处理再次分 frame 而自锁；其它 CPU仍可竞争。

不得在 allocator 锁内：清零整页、分配 `Box/Vec`、页表 walk 中再次 alloc、打印可能分配/取 console 反向锁的复杂日志、等待 scheduler。`frame_allocator_cell()` 暴露原始 cell 只供特殊短批处理；拿到 exclusive_access 后调用 `frame_alloc_result` 会永久重入等待。

idle zero maintenance 使用 try-lock，锁忙就放弃本轮，避免 idle CPU 自旋争抢。锁调试会记录 wait/acquired/released，但诊断 logger 自身不得反向分 frame。

## 零页池

池是固定 `[PhysPageNum;1024]`，不耗动态 Vec；低水位 256、高水位/容量 1024。池中及清零中的页仍在主 allocator 里标记 allocated/ref=1，因此不能同时进入 recycled。`in_flight` 在锁内预占发布槽，防止多个 CPU 合计越界。

需求 miss：预占最多 15 个额外槽，在一次 allocator 锁中批取最多 16 页，锁外清零，返回第 1 页并发布其余。若 raw 分配失败，会先释放 claim，再重试接收并发 idle 刚发布的页。

prefault 只有池长度严格高于 256 才取，不同步清零，保证 ELF BSS 优化不挤占 fault 热路径保底库存。idle 每轮最多取 32 页，锁外清零后发布。

raw `frame_alloc_result` OOM 时会从零页池摘最多 32 页，在不同时持两锁的前提下归还主 allocator，再重试 raw alloc。因此 pool 是可回收缓存，不应被计作永久占用。`frame_mem_stats` 将 pool len 加回 free，但不含正在清零的 `in_flight`，短时间采样可能偏低。

初始化时 `ZeroedFramePool::reset` 只把 len/in_flight 清零，并不逐页 dealloc；随后整个主 allocator init 重置元数据，所以仅在全局无外部 owner时成立。运行期单独 reset pool 会泄漏 owner，当前没有暴露这种入口。

## 地址与安全假设

`zero_frame` 直接写 `PPN * PAGE_SIZE`，依赖可分配 RAM 恒等映射。乘法未 checked；有效平台 PPN应确保可表示。保留区只有一个连续区间：内核镜像、DTB、initrd、多个 reserved-memory 若不组成一段，bring-up 必须裁出管理区或扩展为多区间，不能只传其中一个。

## 修改分配策略时

1. 保留 `allocated/ref_counts` 的诊断语义，或同时替换所有非法释放检测。
2. 统计必须包含 novel 与 recycled 两部分，并扣除保留区。
3. 批量分配中途遇普通 OOM会返回已取得的 partial batch；遇其它错误才回滚本批，调用者必须理解区别。
4. 零页池发布前先清零；预留、清零、入池之间的 `in_flight` 数量必须配平。
5. 不能在持 allocator 锁时调用可能再次分配物理帧的代码。

## 扩展示例：连续 DMA 页

VirtIO 当前循环单页分配并假定 LIFO 返回连续递减 PPN，SMP 插入就失败。应在本 allocator 增加原子 `alloc_contiguous(pages,align)`：同一锁内从 novel 连续段或可证明连续的 free extent 取得；一次性标记每页 ref=1；任一步失败不改变状态。返回 RAII extent，Drop 对称释放全部。不能通过在驱动中暂时关中断解决其它 CPU 并发。

## 回归清单

- 空/反序区间，保留区在头/中/尾/区外/覆盖全部；
- novel 从高到低，跨保留区，recycled LIFO；
- OOM、释放复用、双重/越界/保留/尚未分配页释放；
- ref 1→N→0、未分配 inc_ref、`u32::MAX` overflow；
- batch partial OOM 与非 OOM 回滚；
- 零页 demand/prefault/idle、低高水位、in_flight 多 CPU、raw OOM drain；
- 分配结果全零，回收再分配不承诺零；
- 8 GiB 初始化和 churn 下 heap 元数据曲线；
- 多 CPU 唯一分配、IRQ 重入保护与 nested allocator deadlock 侦测；
- 地址空间 fork/COW/exit 后 free/ref/recycled/heap 全部回归基线。

出现 `drop invalid recycled`、`duplicate recycled`、`novel already allocated` 不能只屏蔽日志；它表示此前生命周期、引用计数或 free-list 已经损坏。
