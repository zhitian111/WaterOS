# Task 02：实现未接线的 per-CPU slab 核心

## 任务目标

在 `runtime-heap-allocator` 内实现一套最小但可单测的 slab 核心：

- 固定 size class；
- 单页 slab；
- per-CPU 本地 freelist；
- slab page header/owner 信息；
- 从抽象 frame source 获取页的能力。

本任务**不接入** `#[global_allocator]`，不影响内核运行路径。

## 实施方案

建议新增子模块：

```text
runtime-heap-allocator/src/slab/
├── mod.rs
├── size_class.rs
├── slab_page.rs
├── cpu_slab.rs
└── slab_cache.rs
```

设计约束：

1. size class 覆盖常见小对象，例如 8/16/32/64/.../2048，具体集合按
   `Layout::size`、`Layout::align` 和 WaterOS 实际分配热点确定。
2. 每块 slab 使用一页，页首保留最小 header，记录：
   - size class；
   - owner CPU；
   - free 对象数量或 bitmap；
   - 回链所需信息。
3. 本地 `CpuSlab` 只被当前 CPU 修改，结合 `CpuLocal<UnsafeCell<...>>` 和
   现有 `with_allocator_interrupt_guard` 使用。
4. 页来源抽象为可注册回调，避免 `runtime-heap-allocator` 反向依赖 `wateros-mm`：

   ```rust
   pub type HeapFrame = usize;
   pub trait HeapFrameSource: Sync {
       fn alloc_frame(&self) -> Option<HeapFrame>;
       fn dealloc_frame(&self, frame: HeapFrame);
   }
   ```

   本任务单元测试使用 fake frame source；任务 03 才注册真实 frame allocator。
5. 页首 header 使用页对齐地址反查：`base = addr & !(PAGE_SIZE - 1)`，在
   `dealloc` 时读 header 判断 owner CPU。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/Cargo.toml`
- 若需要公开测试入口，增加 `self_test` 或 `#[cfg(test)]` 测试模块。

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "CpuLocal current_cpu_id frame_alloc_result"
codegraph impact "CpuLocal"
codegraph explore "with_allocator_interrupt_guard"
```

## 验收方式

优先运行 crate 级单元测试：

```bash
cd /tmp/wateros-kernel-heap-slab/os/components/wateros-runtime
cargo test -p wateros-runtime-heap-allocator --lib
```

若 host 测试因 `wateros-platform-arch` 的目标汇编（RISC-V/LoongArch inline asm）无法
编译，记录该环境限制，并改用 `make rv_check` / `make la_check` 验证 lib 编译；测试
逻辑的运行验证推迟到 Task 03 的 QEMU 冒烟/回归中执行。

再跑两架构 check：

```bash
cd /tmp/wateros-kernel-heap-slab/os
make rv_check
make la_check
git diff --check
```

必须覆盖的用例：

- 每个 size class 的 alloc/dealloc round-trip；
- 单页耗尽后从 fake frame source 再取新页；
- 对象地址能正确反查到 owner CPU 和 size class；
- 空 slab 能触发 `dealloc_frame`；
- 跨 CPU free 的基本接口行为（本任务可先做队列或错误路径）。

## 完成后

新增 `history/02-brief.md`，记录 size class 表、slab header 布局、测试结果和
尚未接入 GlobalAlloc 的边界。
