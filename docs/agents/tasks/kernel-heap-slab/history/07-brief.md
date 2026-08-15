# Task 07 简报：统计、诊断、调参与文档同步

## 完成情况

- 增加 slab 分配/释放计数与 `heap_slab_stats()` 诊断入口；
- 增加 `LARGE_FRAME_ENABLED` 调参开关（默认关闭）并记录宿主机 OOM 原因；
- 同步 `runtime-heap-allocator/README.md`、根 README、任务文档。

## 未完成/后续

- slab 页占用、每 CPU 命中、remote-free 次数尚未全部进 `HeapMemStats`；
- `LARGE_FRAME_ENABLED` 的 A/B 需在稳定宿主机执行；
- 最终性能验收在 Task 09 完成。
