# MM Frame Allocator API 实现说明

对应 API：
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-api/api-v0/src/lib.rs`

## 目标

为 MM 子系统提供稳定的“物理页帧分配/回收”能力，作为 `mm-api` 与 `impl-sv39` 的底层依赖。

## 已定义接口

- `FrameAllocError`
  - `OutOfMemory`
  - `InvalidFrame`
  - `Unsupported`
- `FrameAllocResult<T>`
- `PhysicalFrameAllocator`
  - `type FrameId: Copy + Eq`
  - `alloc_frame(&mut self) -> FrameAllocResult<Self::FrameId>`
  - `dealloc_frame(&mut self, frame: Self::FrameId) -> FrameAllocResult<()>`

## 实现方必须完成

1. `alloc_frame` 行为
   - 有可用页帧时返回唯一帧 ID。
   - 无可用页帧时返回 `OutOfMemory`。
2. `dealloc_frame` 行为
   - 释放已分配帧成功。
   - 对非法帧或重复释放，至少返回 `InvalidFrame`（或明确文档说明早期阶段暂不检测）。
3. 初始化行为
   - 需要提供实现层初始化入口（例如 `init(start_ppn, end_ppn)`）。
   - 范围语义建议固定为 `[start, end)`。
4. 并发/可变借用安全
   - 全局单例应通过安全容器保护（当前项目已有 `wateros-base::sync::UniprocessorSafeCell`）。

## 日志与自检要求

- 日志统一使用 `log` 宏（`log::trace!/info!/warn!`）。
- 推荐提供 `test_with_range(start_ppn, end_ppn)`，至少覆盖：
  - 分配直到耗尽
  - 耗尽后返回 `OutOfMemory`
  - 回收后可再次分配

## 与其他模块的约束

- 建议 `FrameId` 与 `mm-api::addr::PhysPageNum` 对齐，减少上层转换成本。
- 如果实现使用 `BasePPN` 初始化，需在实现内部完成 `BasePPN <-> PhysPageNum` 转换。
