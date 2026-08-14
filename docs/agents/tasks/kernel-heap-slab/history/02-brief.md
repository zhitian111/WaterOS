# Task 02 简报：per-CPU slab 核心

## 完成情况

完成未接线实现。已新增固定 size class、单页 slab header、intrusive free list、
每 CPU 本地 cache 和 `HeapFrameSource` 抽象；`SlabAllocator` 尚未接入 GlobalAlloc。

## 改动文件

- `runtime-heap-allocator/src/slab/mod.rs`
- `runtime-heap-allocator/src/slab/size_class.rs`
- `runtime-heap-allocator/src/slab/slab_page.rs`
- `runtime-heap-allocator/src/slab/slab_cache.rs`
- `runtime-heap-allocator/src/slab/cpu_slab.rs`
- `runtime-heap-allocator/src/lib.rs`

## 关键设计

- size class 表：8/16/24/32/48/64/96/128/192/256/384/512/768/1024/1536/2048；
- 单页 slab，页首 64 字节 header 记录 magic/size_class/owner_cpu/对象计数/free list；
- 本地 cache 维护 current/partial 页，空页归还 frame source；
- `HeapFrameSource` 返回页对齐内核基址，便于 Task 03 适配 frame allocator。

## 验收命令与结果

```text
make rv_check                    PASS
make la_check                    PASS
make kernel-rv-final             PASS
make kernel-la-final             PASS
git diff --check                 PASS
```

## 环境限制

- host `cargo test -p wateros-runtime-heap-allocator --lib` 无法运行：`wateros-platform-arch`
  的 RISC-V inline asm 在 x86 host 上编译失败（`sbi-rt` invalid register）；
- `cargo check --tests --target riscv64gc-unknown-none-elf` 因 no_std 目标无 `test` crate
  无法链接；
- 已按任务文档 fallback 改为两架构 `cargo check` + final 构建；测试逻辑的运行验证推迟到
  Task 03 QEMU 冒烟。

## 未验证项/风险

- slab 单测尚未实际执行；
- 跨核 free、partial/empty 完整策略在 Task 04 完善；
- `SlabAllocator` 尚未接入 GlobalAlloc。
