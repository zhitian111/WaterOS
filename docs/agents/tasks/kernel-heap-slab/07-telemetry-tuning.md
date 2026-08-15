# Task 07：统计、诊断、调参与文档同步

## 任务目标

为新的 slab 路径补齐可观测性和可调参数，并根据中期性能数据做一次系统调参。所有
影响 feature、Makefile、README 或组件文档的变化必须同步。

## 实施方案

1. 统计接口从 TLSF 估算升级为：

   - boot backend 使用字节/空闲字节；
   - slab 使用分配/释放计数、页占用、每 CPU 缓存命中、remote-free 次数；
   - `HeapMemStats` 需要保持 dashboard 和 OOM 日志兼容，必要时增加字段但不得破坏
     现有调用方。

2. 增加受 feature/级别控制的诊断日志：

   - 本核 slab 命中/未命中；
   - frame source 申请/归还次数；
   - 大对象回退次数；
   - 锁等待（若保留共享 partial 锁）。

3. 调参项至少包括：

   - size class 集合；
   - 每 cache 的 partial 页上限；
   - remote-free 队列 drain 阈值；
   - 大对象走连续帧还是 boot TLSF 的阈值。

4. 用 task 00/中间任务日志对比调参前后 `elapsed_s`，保留至少一轮完整 buildstorm 证明
   功能仍通过。

5. 文档同步：

   - `os/components/wateros-runtime/runtime-heap-allocator/README.md`
   - `os/components/wateros-runtime/README.md`
   - `docs/agents/tasks/kernel-heap-slab/README.md`
   - 如改动 feature 传播，更新根 `README.md` 和 `docs/tools/makefile.md`

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-runtime/runtime-heap-allocator/README.md`
- `os/components/wateros-runtime/README.md`
- 可能涉及 `os/Cargo.toml`、`os/components/wateros-runtime/Cargo.toml`、`os/Makefile`
- 根 `README.md`、`docs/tools/makefile.md` 按实际变化同步

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "heap_mem_stats HeapMemStats"
codegraph impact "HeapMemStats"
codegraph callers "heap_mem_stats"
codegraph explore "frame_mem_stats"
```

## 验收方式

```bash
cd /tmp/wateros-kernel-heap-slab/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
git diff --check
```

运行时：

- 两架构至少各 1 轮完整 buildstorm；
- `heap_mem_stats`、`frame_mem_stats` 和 dashboard 不再出现负数或溢出；
- 新增诊断日志默认不进入热路径；`stall-debug`/tuning feature 下可观测。

所有 QEMU 运行均加 `-snapshot`。

## 完成后

新增 `history/07-brief.md`，记录统计字段变化、调参结果、文档同步清单和最终
参数快照。
