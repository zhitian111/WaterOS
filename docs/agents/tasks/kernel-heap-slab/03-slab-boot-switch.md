# Task 03：接入 boot TLSF + frame-backed slab 切换

## 任务目标

把 task 02 的 slab 核心接入内核 GlobalAlloc 门面。启动早期仍使用静态 boot TLSF，
frame allocator 初始化完成后切换到 slab；先只让**小对象**走每核 slab，大对象继续
由 boot TLSF 服务。

这是第一个改变运行时分配路径的任务，必须在两架构上都做运行时回归。

## 实施方案

1. 在 `runtime-heap-allocator` 门面中增加后端状态：

   - `boot backend`：沿用现有 TLSF/链表后端和 `HEAP_SPACE`；
   - `slab backend`：task 02 的 slab，使用注册的 `HeapFrameSource`；
   - 运行期活动后端由 `OnceCell`/原子状态表示，BSP 完成 frame allocator 后再切换。

2. 保持启动顺序安全：

   - `runtime::heap_allocator::init()` 仍只初始化 boot backend；
   - 在 `mm::init_after_boot()` 完成后，由顶层 `os/src/main.rs` 注册真实 frame source
     并调用 `runtime::heap_allocator::activate_slab()`；
   - 切换前所有分配仍走 boot TLSF；切换后新分配按 size class 路由。

3. 定义真实 frame source 适配器，建议放在 `os/src/main.rs` 或一个很小的 bring-up
   适配文件，只调用 `mm::frame_alloctor::frame_alloc_result()` /
   `frame_dealloc_result()`，不让 `runtime-heap-allocator` 直接依赖 `wateros-mm`。

4. 路由规则：

   ```text
   layout.size() <= SLAB_MAX && layout.align() <= SLAB_MAX
     -> current_cpu_id() 对应的 slab cache
   otherwise
     -> boot backend
   ```

5. `dealloc`/`realloc` 必须能区分分配来自 slab 还是 boot backend：

   - slab 对象通过页首 header 识别；
   - 其余指针回落到 boot backend；
   - 对无法识别的指针按现有 `dealloc_pointer_in_heap` 逻辑处理，不能静默误释放。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-runtime/runtime-heap-allocator/Cargo.toml`
- `os/components/wateros-runtime/Cargo.toml`
- `os/src/main.rs`：注册 frame source 并切换后端
- 可能新增 `os/src/heap_frame_source.rs` 或等价适配文件

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "heap_allocator::init mm::init_after_boot frame_alloc_result"
codegraph impact "heap_allocator::init"
codegraph callers "init_after_boot"
codegraph explore "CpuLocal current_cpu_id"
```

## 验收方式

静态：

```bash
cd /tmp/wateros-kernel-heap-slab/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
git diff --check
```

运行时：

1. 用当前镜像准备脚本跑至少一轮完整 buildstorm：
   - LA：QEMU 9.2.1，`-m 36G -smp 12`；
   - RV：QEMU 9.2.1，`-m 16G -smp 8`。
2. 必须满足功能验收：

   ```text
   TOOLCHAIN_RESULT status=OK
   MINIBUILD_RESULT status=OK
   BUILDSTORM_RESULT mode=multi status=OK rc=0 ... run=OK
   all commands finished
   ```

3. 日志中不得出现：

   ```text
   [heap] OOM
   [heap] recursive heap allocation detected
   panic
   invalid TLSF dealloc
   ENOMEM
   ```

4. 若某一架构无法完整通过，本任务不能提交，必须先定位并修复。

## 完成后

新增 `history/03-brief.md`，记录切换时机、真实 frame source 位置、两架构运行结果
和性能初值。
