# Frame Allocator API v0

[物理帧分配器](../../../README.md)

这里是物理帧分配器的最小、实现无关契约。它定义
`PhysicalFrameAllocator`、`FrameAllocError`、`FrameAllocResult` 与 `FrameMemStats`，不包含
全局锁、引用计数、页表格式、RAM 区间发现或实际清零方法。当前 WaterOS 实现把
`FrameId` 绑定为 `PhysPageNum`，一帧通常是 4 KiB。

## 1. 为什么单独有 API crate

MM 的页表、fault、`mmap`/`brk` 逻辑只需要“拿一帧/还一帧”，不应依赖栈式 free list 的
内部表示。通过泛型参数：

```rust
fn map_one<A>(allocator: &mut A) -> FrameAllocResult<()>
where
    A: PhysicalFrameAllocator<FrameId = PhysPageNum>,
{
    let frame = allocator.alloc_frame()?;
    // 安装映射；失败路径必须 allocator.dealloc_frame(frame)
    Ok(())
}
```

同一套地址空间算法可以接真实的 `GlobalPhysFrameAllocator`，也可在线下单元测试中接一个
小容量假 allocator，稳定制造 OOM 和回滚场景。

依赖方向是：

```text
mm-api / mm-impl
  -> frame-alloctor-api-v0（本 crate）
     <- impl-stack 实现 trait
        <- frame-alloctor 聚合层提供全局入口、RAII、引用计数和零页池
```

目录名和 crate 历史上使用了 `alloctor` 拼写；离线改题时不要自行改名，否则会连锁改变
workspace、feature 和 import 路径。

## 2. trait 精确契约

### `type FrameId: Copy + Eq`

ID 是可复制的标识，不代表复制所有权。拿到两个相同 `FrameId` 并不自动产生两个物理页
引用；共享映射必须使用聚合实现显式提供的引用计数接口。

### `alloc_frame(&mut self)`

成功时把一帧的一个所有权交给调用者。该帧在归还前不得再次成功分配。耗尽应返回
`OutOfMemory`，调用方必须能够撤销此前已经分配/安装的页；trait 没有承诺物理连续、
清零、分配顺序或恒等映射。

### `try_alloc_zeroed_frame(&mut self)`

返回值有三层语义：

- `Ok(Some(id))`：成功转移一帧正常所有权，且返回时整帧字节均为零；
- `Ok(None)`：该 allocator 不支持可选的预清零快路径，调用方应 `alloc_frame` 后自行清零；
- `Err(OutOfMemory)`：allocator 支持/尝试该操作但无法提供帧，不能当作“不支持”。

默认实现永远返回 `Ok(None)`，所以新增简单 allocator 时可以不实现该方法。用户可见页
不能因为得到 `None` 就跳过清零，否则会泄漏旧内核或进程数据。

### `dealloc_frame(&mut self, frame)`

成功后调用者失去这一所有权，不得继续通过该 ID 访问或再次释放。非法、保留、从未
分配或重复释放应返回 `InvalidFrame`，至少不能污染 free list。基础 trait 按“一次成功
分配对应一次成功回收”理解；WaterOS 全局实现的共享引用计数是实现侧扩展，不可悄悄
改变独立 trait 实现的测试语义。

trait 通过 `&mut self` 串行访问，但没有承诺实现本身是 `Send`/`Sync` 或关中断。全局实现
负责在外层建立 SMP 互斥与中断纪律；局部测试 allocator 无需复制这些机制。

## 3. 错误和回滚

| 错误 | 含义 | 调用方动作 |
|---|---|---|
| `OutOfMemory` | 当前无可分配帧 | 释放本次操作已获得的帧并返回 `ENOMEM` 等上层错误 |
| `InvalidFrame` | 回收目标不属于有效的已分配集合 | 视为所有权 bug，不能再次盲目入栈 |
| `Unsupported` | 某实现明确不支持该操作 | 仅用于真实操作错误；zeroed 快路径“不提供”应使用 `Ok(None)` |

典型多级页表创建必须记录本次新分配的每一帧。若第 N 次分配 OOM，应先撤销已写 PTE，
再以安全顺序归还前 N-1 帧；不能只返回错误，否则会形成随失败次数增长的物理页泄漏。
映射已经对其他 CPU 可见时，还必须按架构要求完成 TLB shootdown 后再回收物理帧。

## 4. `FrameMemStats`

统计对象是一次只读快照：

- `total_frames`：allocator 管理的总帧数；
- `free_frames`：快照时可直接分配的空闲帧数；
- `page_bytes`：单帧字节数；
- `total_bytes()` / `free_bytes()` 使用饱和乘法；
- `used_bytes()` 使用饱和减法，异常统计不会发生整数回绕。

API 没有把“获取统计”放进 trait，实际由聚合层 `frame_mem_stats()` 提供。因此自定义
allocator 可以只实现分配契约。WaterOS 的零页池可能持有已从主池分配的帧，分析
`free_frames` 时要同时看实现侧零页池库存。

`/proc/meminfo` 中物理页统计和 runtime heap 不是一回事：物理页很多仍可能发生内核
heap OOM；扩大 QEMU RAM 也不会自动扩大固定大小的 heap。

## 5. 所有权生命周期

```text
free
  -- alloc_frame / Ok(Some zeroed)) --> owned(ref=1 in current impl)
  -- dealloc_frame -----------------> free
```

基础 API 没有 RAII。真实内核优先使用聚合层的 `OwnedPhysPage` 或明确的 rollback guard，
避免 `?` 提前返回漏页。裸 `FrameId` 可以用于 PTE 编码，但它不携带 Drop 行为。

以下页面不能直接交给 allocator 回收：MMIO 页、内核/DTB 保留页、借用的共享缓存页、
仍被其他 PTE 引用的页。地址空间销毁的正确顺序通常是停止新访问、移除映射、完成必要
TLB 同步、减少/释放对应所有权，最后回收页表页。

## 6. 离线新增实现示例

最小测试实现可以用固定数组记录状态：

```rust
struct TinyAllocator {
    used: [bool; 8],
}

impl PhysicalFrameAllocator for TinyAllocator {
    type FrameId = usize;

    fn alloc_frame(&mut self) -> FrameAllocResult<usize> {
        let id = self.used.iter().position(|used| !*used)
            .ok_or(FrameAllocError::OutOfMemory)?;
        self.used[id] = true;
        Ok(id)
    }

    fn dealloc_frame(&mut self, id: usize) -> FrameAllocResult<()> {
        let used = self.used.get_mut(id).ok_or(FrameAllocError::InvalidFrame)?;
        if !*used { return Err(FrameAllocError::InvalidFrame); }
        *used = false;
        Ok(())
    }
}
```

这个实现故意保留默认 `try_alloc_zeroed_frame -> Ok(None)`。若要支持 zeroed，测试内存
必须真的可寻址并在返回前全部清零，不能只靠状态位声称为零。

## 7. 修改 API 的影响面

新增必需 trait 方法会破坏所有实现；优先提供有安全语义的默认实现。新增错误变体或统计
字段时至少检查：

1. `frame-alloctor/src/lib.rs` 聚合再导出；
2. `frame-alloctor-impl/impl-stack`；
3. Sv39 与 LoongArch64 的 pagetable、fault、user access、`mmap`/`brk`；
4. 错误到 syscall errno 的映射；
5. procfs `/proc/meminfo` 与 dashboard；
6. 对错误做穷举匹配的单元测试或诊断代码。

## 8. 自回归矩阵

基础 trait 测试应覆盖：

- 分配出的 ID 在未释放时唯一；
- 分配至容量上限返回 `OutOfMemory` 而非越界或 panic；
- 回收后容量恢复并可复用；
- 越界、未分配和双重回收返回 `InvalidFrame` 且不增加容量；
- 默认 zeroed 路径返回 `Ok(None)`；支持 zeroed 的实现逐字节验证整页；
- N 次分配中途失败后，调用方回滚使基线完全恢复；
- 真实全局实现并发分配不重复，保留区永不返回；
- 地址空间反复创建、fault、unmap、exit 后 `free_frames` 回到稳定基线。

从 `os/` 做集成检查：

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```
