# Task 03：增加低扰动 slab 诊断

## 任务目标

在不改变默认性能的前提下，量化实际 workload 的 size class、local hit、remote free、
remote drain 长度、frame refill 和每 CPU/page 占用，为后续结构改造提供证据。

## 实施方案

1. 新增独立 `slab-diagnostics` feature；默认 Final 完全裁剪诊断代码和符号。
2. 统计写入 per-CPU、cache-line 对齐槽位；allocator guard 关闭本地中断时使用普通
   load/store 或低频采样，禁止全局 `fetch_add`。
3. 汇总只在 bring-up 结束或显式诊断点执行，不逐次串口打印。
4. 记录 16 个当前 size class、fallback、OOM、remote CAS miss、drain 数量/最大长度、
   每 class 页数和空页数。

## 验收方式

```bash
cd os
make rv_check
make la_check
make check ARCH=rv PROFILE=final EXTRA_FEATURES=slab-diagnostics
make check ARCH=la PROFILE=final EXTRA_FEATURES=slab-diagnostics
git diff --check
```

普通 Final 必须不含诊断符号；诊断 Final 必须能输出汇总且不改变功能结果。使用 120 秒
BuildStorm 诊断轮确认在同一 marker 前不因统计开销明显停滞；再用完整轮只作数据采集，
不把诊断轮成绩作为性能结论。

## 涉及文件与 CodeGraph

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/src/user_bringup_common.rs` 或现有 bring-up stats 接线

```bash
codegraph explore "SlabCache CpuSlab remote_push drain_remote HeapFrameSource alloc_frame"
codegraph impact "with_allocator_interrupt_guard"
```

## 完成后

新增 `history/03-brief.md`，附诊断输出、采样率、各架构分布及是否满足结构改造门禁。

