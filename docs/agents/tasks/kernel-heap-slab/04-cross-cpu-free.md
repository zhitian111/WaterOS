# Task 04：完善跨核 free、partial/empty slab 管理

## 任务目标

让 slab 在多核释放路径下正确工作：

- 本核 free 走本地 freelist；
- 非 owner CPU free 安全地归还给 owner CPU；
- 满 slab 释放后回到 partial；
- 全空 slab 归还 frame source；
- 避免跨核 free 造成重复释放、丢失对象或长期持有全局锁。

## 实施方案

1. 在 slab page header 中明确 `owner_cpu`，本核判断 `current_cpu_id() == owner_cpu`。
2. 本核 free：

   - 压回本地 freelist；
   - 若 slab 从 full 变为 partial，更新本地 partial 列表；
   - 若对象数归零，把整页归还 `dealloc_frame`。

3. 跨核 free：

   - 初版可以使用 per-cache 的小锁或每 owner CPU 一个 remote-free 队列；
   - 队列中只放待归还对象，owner CPU 在下一次本地 alloc/free 时 drain；
   - 若后续性能数据要求，再升级为 lock-free CAS/MPMC 队列。

4. partial 管理：

   - 每 cache 维护 owner CPU 的 partial 页列表；
   - 当前页耗尽时优先从本核 partial 取页，再向 frame source 申请；
   - 禁止在持有 allocator guard 时执行会再次分配/调度的操作。

5. 正确性不变量：

   - 一个对象同时只存在于一个 freelist 或一个 remote 队列中；
   - `dealloc_frame` 只能在全空且已从所有队列摘除后调用；
   - header 读取必须校验页对齐和 size class，防止错误指针。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/cpu_slab.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/slab_page.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/slab_cache.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/mod.rs`
- 相关 `#[cfg(test)]` 测试

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "CpuSlab SlabPage SlabCache"
codegraph impact "SlabCache"
codegraph explore "with_allocator_interrupt_guard"
```

## 验收方式

单元/压力测试：

```bash
cd /tmp/wateros-kernel-heap-slab/os/components/wateros-runtime
cargo test -p wateros-runtime-heap-allocator --lib
```

必须包含：

- 单核随机大小 alloc/dealloc；
- 模拟两个 CPU 交叉 free；
- 页耗尽、full→partial、partial→empty、empty 归还 frame source；
- 双重释放/错误指针不应静默成功。

内核构建：

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
- 多核压力下无 panic、无 `[heap]` 错误、无 `all commands finished` 缺失。

所有 QEMU 运行均加 `-snapshot`。

已知：RV `-smp 8` 在当前 main baseline 上本身存在间歇性 guest `SIGSEGV`
（同镜像 baseline 对照同样失败）；本任务记录该问题，最终功能验收前必须另行修复。

## 完成后

新增 `history/04-brief.md`，记录 remote-free 实现选择、压力测试结果和两架构
buildstorm 结果。
