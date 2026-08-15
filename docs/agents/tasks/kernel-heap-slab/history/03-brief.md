# Task 03 简报：boot TLSF + frame-backed slab 切换

## 完成情况

完成。`heap-slab` feature 已打通：BSP 在 frame allocator 初始化后注册
`HeapFrameSourceAdapter` 并激活 slab；小对象走每核 slab，大对象和早期分配继续走
boot TLSF。默认 `heap-tlsf` 行为不变。

## 关键修复

- `StackFrameAllocator` 的回收栈从 `Vec` 改为**空闲页首字 intrusive 链表**，
  消除 `dealloc_frame → Vec::grow → GlobalAlloc::realloc → slab → frame_allocator`
  的递归死锁（GDB 定位到该调用链）；
- `SlabPageHeader` 增加 `in_partial` 标志，防止同一 partial 页被重复入链成环；
- slab 对象起始地址按 size class 对齐，避免 `base+64` 破坏大 size class 对齐；
- `KernelAllocator::dealloc` 以 boot HEAP_SPACE 地址范围区分 boot/slab 来源。

## 改动文件

- `os/Cargo.toml`、`os/components/wateros-runtime/Cargo.toml`、
  `runtime-heap-allocator/Cargo.toml`：新增 `heap-slab` / `impl-slab` feature；
- `runtime-heap-allocator/src/lib.rs`：`KernelAllocator` 运行期状态与路由；
- `runtime-heap-allocator/src/slab/**`：接入与正确性修复；
- `mm-frame-alloctor/impl-stack/src/lib.rs`：回收链表改为 intrusive；
- `os/src/main.rs`：frame source 适配与激活；
- `README.md`、组件 README、任务文档同步。

## 验收命令与结果

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final  PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final  PASS
git diff --check                                 PASS
```

运行时（`-smp 1`，真实 pub 镜像 + recovered buildstorm 脚本，`-snapshot`）：

| 架构 | TOOLCHAIN | MINIBUILD | BUILD_STORM | elapsed_s | run | all finished |
|---|---|---|---|---|---|---|
| RV | OK | OK | OK rc=0 | 1569.48 | OK | 是 |
| LA | OK | OK | OK rc=0 | 1186.29 | OK | 是 |

- RV 日志：`/tmp/wateros-slab-rv-task03-real-smp1.log`，
  SHA-256 `fd462d9be68c4ef90f4e50df1a5ff42fe4f4fe7a6039ef58db213841c9e7946a`
- LA 日志：`/tmp/wateros-slab-la-task03-real-smp1.log`，
  SHA-256 `639c4c3ecc21d8ab93da8d201f3d394930a5a06c23ca233dcd080ebb42913316`

## 未验证项

- 线上 `-smp 8/12` 多核完整回归尚未执行；跨核 free 会在 Task 04 实现后验证；
- `heap_mem_stats` 暂只反映 boot TLSF，slab 统计在 Task 07 补齐。
