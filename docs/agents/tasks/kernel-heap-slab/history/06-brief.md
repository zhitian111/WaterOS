# Task 06 简报：大对象连续帧路径

## 完成情况

完成连续帧分配基础设施：

- `StackFrameAllocator::alloc_contiguous(pages)`：从 `next_novel` 高段分配连续页；
- `HeapFrameSource` 增加 `alloc_contiguous` / `dealloc_contiguous`；
- `mm-frame-alloctor` 导出 `frame_alloc_contiguous_result`；
- `KernelAllocator` 支持大对象连续帧路由，但默认 `LARGE_FRAME_ENABLED=false`，
  大对象继续走 boot TLSF。

## 默认关闭原因

开启连续帧大对象后，LA `-m 36G` 完整 buildstorm 两次在编译阶段被宿主机 OOM kill
（QEMU exit 137）；guest 会把大量帧提交到 QEMU 内存。默认关闭可保持功能稳定，
后续调参按环境决定是否启用。

## 验收

- `HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check` / `la_check` PASS；
- LA `-smp 12` 完整 buildstorm（大对象回退 boot TLSF）：
  `status=OK rc=0 elapsed_s=595.21 run=OK`，
  日志 `/tmp/wateros-slab-la-task06-smp12-3.log`，
  SHA-256 `8b8b75d548aadd8a6a119ac48e57d7594b3f6e689fbcb2f5eab1cb45337159da`。

## 未验证项

- 连续帧大对象路径的启用值未在稳定宿主机上完成 A/B；
- `dealloc_contiguous` 逐帧归还性能待优化。
